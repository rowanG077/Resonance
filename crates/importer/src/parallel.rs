//! Bounded parallel work within one pass; the caller owns pass ordering.
use anyhow::Result;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

pub(crate) fn for_each<T: Sync>(
    items: &[T],
    work: impl Fn(usize, &T) -> Result<()> + Sync,
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let workers = thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(items.len());
    eprintln!("  Using {workers} worker threads for {} jobs", items.len());
    if workers <= 1 {
        return items
            .iter()
            .enumerate()
            .try_for_each(|(i, item)| work(i, item));
    }
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        let workers = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            return Ok(());
                        };
                        if let Err(error) = work(index, item) {
                            next.store(items.len(), Ordering::Relaxed);
                            return Err(error);
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        workers.into_iter().try_for_each(|worker| {
            worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    })
}
