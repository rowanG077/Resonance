use super::*;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[test]
fn appending_retained_endpoints_preserves_storage_until_both_are_released() {
    let mut arena = Arena::new(4096).unwrap();
    let calls = system_calls();
    let first = arena.run(|| vec![7u32; 32]).unwrap();
    let address = first.as_ptr();
    let first_used = arena.used();
    let second = arena.append(|| vec![9u32; 16]).unwrap();
    assert!(arena.used() > first_used);
    assert_eq!(first, [7; 32]);
    assert_eq!(second, [9; 16]);
    assert_eq!(arena.run(|| ()), Err(Error::StillInUse));
    drop(first);
    assert_eq!(arena.run(|| ()), Err(Error::StillInUse));
    drop(second);
    let reused = arena.run(|| vec![11u32; 32]).unwrap();
    assert_eq!(reused.as_ptr(), address);
    drop(reused);
    assert_eq!(system_calls(), calls);
}

#[test]
fn repeated_nested_collections_reuse_storage_without_system_calls() {
    let mut arena = Arena::new(1024 * 1024).unwrap();
    let before = system_calls();
    let mut first_address = None;
    for _ in 0..100 {
        let values = arena
            .run(|| {
                let mut values = std::collections::BTreeMap::new();
                for key in 0..100 {
                    values.insert(key, Arc::new(vec![key; 32]));
                }
                values
            })
            .unwrap();
        let address = values[&0].as_ptr();
        assert_eq!(*first_address.get_or_insert(address), address);
        drop(values);
    }
    assert_eq!(system_calls(), before);
    assert!(arena.used() > 0);
}

#[test]
fn retained_values_block_reset_and_can_outlive_the_arena() {
    let mut arena = Arena::new(4096).unwrap();
    let bytes = arena.run(|| vec![42_u8; 100]).unwrap();
    assert_eq!(arena.run(|| ()).unwrap_err(), Error::StillInUse);
    drop(arena);
    assert_eq!(bytes[99], 42);
    std::thread::spawn(move || drop(bytes)).join().unwrap();
}

#[test]
fn alignment_zeroing_reallocation_and_exhaustion_respect_allocator_contracts() {
    let mut arena = Arena::new(64 * 1024).unwrap();
    let before = system_calls();
    arena
        .run(|| {
            for align in [1, 2, 8, 64, 4096] {
                let layout = Layout::from_size_align(31, align).unwrap();
                // SAFETY: Only the returned live allocation is accessed and freed.
                unsafe {
                    let data = ALLOCATOR.alloc_zeroed(layout);
                    assert!(!data.is_null());
                    assert_eq!(data.addr() % align, 0);
                    assert!(std::slice::from_raw_parts(data, 31).iter().all(|&b| b == 0));
                    data.write(123);
                    let next = ALLOCATOR.realloc(data, layout, 77);
                    assert!(!next.is_null());
                    assert_eq!(next.addr() % align, 0);
                    assert_eq!(next.read(), 123);
                    ALLOCATOR.dealloc(next, Layout::from_size_align(77, align).unwrap());
                }
            }
            let huge = Layout::from_size_align(1024 * 1024, 8).unwrap();
            // SAFETY: Inspect failure without the infallible collection OOM handler.
            assert!(unsafe { ALLOCATOR.alloc(huge) }.is_null());
        })
        .unwrap();
    assert_eq!(system_calls(), before);
}

#[test]
fn system_reallocation_preserves_header_and_unwinding_restores_allocator() {
    let mut values = vec![7_u64; 16];
    values.resize(1000, 8);
    assert_eq!(values[15], 7);
    assert_eq!(values[999], 8);
    let mut arena = Arena::new(65536).unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = arena.run(|| {
            let _value = vec![1; 100];
            panic!("scope test")
        });
    }));
    assert!(failure.is_err());
    drop(failure);
    assert!(ACTIVE.get().is_null());
}

#[test]
fn reallocating_across_scope_boundaries_preserves_contents_and_releases_ownership() {
    let mut arena = Arena::new(65536).unwrap();
    let mut values = vec![17_u64; 16];
    arena
        .run(|| {
            values.resize(100, 23);
            assert_eq!(values[15], 17);
        })
        .unwrap();
    assert_eq!(arena.run(|| ()).unwrap_err(), Error::StillInUse);
    values.resize(1000, 31);
    assert_eq!(values[15], 17);
    assert_eq!(values[99], 23);
    assert_eq!(values[999], 31);
    arena.run(|| ()).unwrap();
}
