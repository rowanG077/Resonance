//! Reusable, bounded storage for disposable simulation branches.
//!
//! Rust's ordinary Vec/Box/BTreeMap/Arc allocations are routed into the active
//! thread's arena. Every allocation retains its region until deallocation, so
//! even a value escaping the scope is safe: reset refuses to overwrite it.
//! Outside a scope, allocations use System with a one-pointer ownership header.
//! Arena exhaustion returns null (ordinary Rust allocation failure), never a
//! hidden heap fallback. Reserve capacity before entering the frame loop.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

static INSTALLED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static ACTIVE: Cell<*const Region> = const { Cell::new(ptr::null()) };
    static SYSTEM_CALLS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// Thread-local system allocation/deallocation counts, including reallocation.
pub fn system_calls() -> (usize, usize) {
    SYSTEM_CALLS.get()
}
fn count_system(allocate: bool) {
    let _ = SYSTEM_CALLS.try_with(|calls| {
        let (a, d) = calls.get();
        calls.set((
            a.wrapping_add(usize::from(allocate)),
            d.wrapping_add(usize::from(!allocate)),
        ));
    });
}

#[repr(C)]
struct Header {
    region: *const Region,
}
const HEADER: usize = size_of::<Header>();

fn system_layout(layout: Layout) -> Option<(Layout, usize)> {
    let align = layout.align().max(align_of::<Header>());
    let offset = HEADER.checked_add(align - 1)? & !(align - 1);
    Some((
        Layout::from_size_align(offset.checked_add(layout.size())?, align).ok()?,
        offset,
    ))
}

/// Install once with `#[global_allocator]` in the executable (or its owner crate).
pub struct Allocator;

// SAFETY: Both paths respect Layout alignment/size. Every returned pointer has
// an initialized aligned header. A region reference is retained per allocation;
// System pointers are freed using the exact padded layout used at allocation.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Avoid a shared atomic write on every allocation from render workers.
        if !INSTALLED.load(Ordering::Relaxed) {
            INSTALLED.store(true, Ordering::Relaxed);
        }
        let active = ACTIVE.try_with(Cell::get).unwrap_or(ptr::null());
        if !active.is_null() {
            // SAFETY: The active scope holds an Arc reference to this region.
            return unsafe { (*active).allocate(active, layout) };
        }
        let Some((padded, offset)) = system_layout(layout) else {
            return ptr::null_mut();
        };
        count_system(true);
        // SAFETY: padded is a valid nonzero layout; null is propagated.
        let base = unsafe { System.alloc(padded) };
        if base.is_null() {
            return base;
        }
        // SAFETY: offset reserves an aligned header and the complete payload.
        unsafe {
            let data = base.add(offset);
            data.sub(HEADER).cast::<Header>().write(Header {
                region: ptr::null(),
            });
            data
        }
    }

    unsafe fn dealloc(&self, data: *mut u8, layout: Layout) {
        // SAFETY: GlobalAlloc requires a live pointer from this allocator.
        let region = unsafe { (*data.sub(HEADER).cast::<Header>()).region };
        if !region.is_null() {
            // SAFETY: alloc retained exactly one Arc reference for this block.
            // No access to the header/region occurs after releasing that reference.
            unsafe { Arc::decrement_strong_count(region) };
        } else if let Some((padded, offset)) = system_layout(layout) {
            count_system(false);
            // SAFETY: Recover the original allocation and its exact layout.
            unsafe { System.dealloc(data.sub(offset), padded) };
        }
    }

    unsafe fn realloc(&self, data: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let Ok(next_layout) = Layout::from_size_align(size, layout.align()) else {
            return ptr::null_mut();
        };
        // SAFETY: data/layout describe a live allocation of this allocator.
        let region = unsafe { (*data.sub(HEADER).cast::<Header>()).region };
        let active = ACTIVE.try_with(Cell::get).unwrap_or(ptr::null());
        if region.is_null() && active.is_null() {
            let (Some((old, offset)), Some((new, _))) =
                (system_layout(layout), system_layout(next_layout))
            else {
                return ptr::null_mut();
            };
            count_system(true);
            // SAFETY: Alignment and header offset are unchanged; old is the
            // original padded layout. System preserves the ownership header.
            let base = unsafe { System.realloc(data.sub(offset), old, new.size()) };
            return if base.is_null() {
                base
            } else {
                unsafe { base.add(offset) }
            };
        }
        // SAFETY: Allocate a disjoint block; on failure the old block stays live.
        let next = unsafe { self.alloc(next_layout) };
        if !next.is_null() {
            unsafe {
                ptr::copy_nonoverlapping(data, next, layout.size().min(size));
                self.dealloc(data, layout);
            }
        }
        next
    }
}

struct Region {
    data: NonNull<u8>,
    layout: Layout,
    cursor: AtomicUsize,
}
// SAFETY: Cursor reservations are atomic and never overlap. Reset requires sole
// ownership; the region cannot be freed while any allocated block survives.
unsafe impl Send for Region {}
unsafe impl Sync for Region {}
impl Region {
    unsafe fn allocate(&self, owner: *const Self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(align_of::<Header>());
        let base = self.data.as_ptr().addr();
        let reserve = |cursor: usize| {
            let address = base.checked_add(cursor)?.checked_add(HEADER)?;
            let data = address.checked_add(align - 1)? & !(align - 1);
            let end = data.checked_sub(base)?.checked_add(layout.size())?;
            (end <= self.layout.size()).then_some(end)
        };
        let Ok(start) = self
            .cursor
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, reserve)
        else {
            return ptr::null_mut();
        };
        let address = (base + start + HEADER + align - 1) & !(align - 1);
        // SAFETY: The successful reservation checked bounds/overflow/alignment.
        // owner came from Arc::into_raw in the active scope and is still live.
        unsafe {
            let data = self.data.as_ptr().add(address - base);
            Arc::increment_strong_count(owner);
            data.sub(HEADER)
                .cast::<Header>()
                .write(Header { region: owner });
            data
        }
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        count_system(false);
        // SAFETY: All allocation references and the arena owner have gone away.
        unsafe { System.dealloc(self.data.as_ptr(), self.layout) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    AllocatorNotInstalled,
    InvalidCapacity,
    OutOfMemory,
    StillInUse,
    NestedScope,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "simulation arena: {self:?}")
    }
}
impl std::error::Error for Error {}

pub struct Arena {
    region: Arc<Region>,
}
impl Arena {
    pub fn new(capacity: usize) -> Result<Self, Error> {
        if !INSTALLED.load(Ordering::Relaxed) {
            return Err(Error::AllocatorNotInstalled);
        }
        if !ACTIVE.get().is_null() {
            return Err(Error::NestedScope);
        }
        let layout = Layout::from_size_align(capacity, 64).map_err(|_| Error::InvalidCapacity)?;
        if capacity == 0 {
            return Err(Error::InvalidCapacity);
        }
        count_system(true);
        // SAFETY: Nonzero valid layout; allocation failure is returned to caller.
        let data = NonNull::new(unsafe { System.alloc(layout) }).ok_or(Error::OutOfMemory)?;
        Ok(Self {
            region: Arc::new(Region {
                data,
                layout,
                cursor: AtomicUsize::new(0),
            }),
        })
    }

    pub fn used(&self) -> usize {
        self.region.cursor.load(Ordering::Relaxed)
    }
    pub fn capacity(&self) -> usize {
        self.region.layout.size()
    }

    /// Reset and run in previously reserved storage. Retaining a previous result
    /// is safe but prevents reuse until all its allocations have been released.
    pub fn run<T>(&mut self, work: impl FnOnce() -> T) -> Result<T, Error> {
        if !ACTIVE.get().is_null() {
            return Err(Error::NestedScope);
        }
        if Arc::strong_count(&self.region) != 1 {
            return Err(Error::StillInUse);
        }
        self.region.cursor.store(0, Ordering::Relaxed);
        self.append(work)
    }

    /// Allocate additional values alongside a retained result without rewinding
    /// any live storage. The same fixed capacity and scope restrictions apply.
    pub fn append<T>(&mut self, work: impl FnOnce() -> T) -> Result<T, Error> {
        if !ACTIVE.get().is_null() {
            return Err(Error::NestedScope);
        }
        let owner = Arc::into_raw(self.region.clone());
        ACTIVE.set(owner);
        struct Reset(*const Region);
        impl Drop for Reset {
            fn drop(&mut self) {
                ACTIVE.set(ptr::null());
                // SAFETY: Releases the reference retained by into_raw above.
                unsafe { Arc::decrement_strong_count(self.0) };
            }
        }
        let _reset = Reset(owner);
        Ok(work())
    }
}

#[cfg(test)]
mod tests;
