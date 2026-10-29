//! Bounded parallel work within one pass; the caller owns pass ordering.
use anyhow::Result;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

pub(crate) fn for_each<T: Sync>(
    items: &[T],
    work: impl Fn(usize, &T) -> Result<()> + Sync,
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let workers = workers().min(items.len());
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

/// CPU allowance for one bounded batch, respecting process affinity and quotas.
pub(crate) fn workers() -> usize {
    thread::available_parallelism().map_or(1, |count| count.get())
}

/// Convert a bounded batch concurrently and retain input order for publication.
pub(crate) fn map<T: Sync, R: Send>(
    items: &[T],
    work: impl Fn(usize, &T) -> Result<R> + Sync,
) -> Result<Vec<R>> {
    let output = Mutex::new((0..items.len()).map(|_| None).collect::<Vec<_>>());
    for_each(items, |index, item| {
        let value = work(index, item)?;
        output.lock().unwrap()[index] = Some(value);
        Ok(())
    })?;
    Ok(output
        .into_inner()
        .unwrap()
        .into_iter()
        .map(Option::unwrap)
        .collect())
}
