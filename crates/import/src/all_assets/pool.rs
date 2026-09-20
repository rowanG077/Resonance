//! One bounded queue; workers keep their decoder state and never create nested pools.
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

pub(super) fn map<T: Sync, U: Send, S>(
    jobs: &[T],
    workers: usize,
    init: impl Fn() -> S + Sync,
    run: impl Fn(&mut S, &T) -> U + Sync,
) -> Vec<U> {
    let next = AtomicUsize::new(0);
    let mut results = thread::scope(|scope| {
        let handles = (0..workers.min(jobs.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut state = init();
                    let mut results = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(index) else { break };
                        results.push((index, run(&mut state, job)));
                    }
                    results
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("asset worker panicked"))
            .collect::<Vec<_>>()
    });
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn workers_overlap_and_return_every_result_in_input_order() {
        let barrier = Barrier::new(2);
        let jobs = (0..32).collect::<Vec<_>>();
        let results = map(
            &jobs,
            2,
            || 0,
            |calls, &id| {
                if id < 2 {
                    barrier.wait();
                }
                *calls += 1;
                id * id
            },
        );
        assert_eq!(results, jobs.iter().map(|id| id * id).collect::<Vec<_>>());
        assert!(map::<usize, usize, _>(&[], 2, || (), |_, id| *id).is_empty());
    }
}

#[cfg(test)]
#[test]
fn concurrent_publication_never_shares_temporary_files() {
    let directory = crate::temporary_path(&std::env::temp_dir().join("resonance-publication"));
    std::fs::create_dir(&directory).unwrap();
    let destination = directory.join("asset.bin");
    let jobs = [1_u8, 2, 3, 4];
    let barrier = std::sync::Barrier::new(jobs.len());
    map(
        &jobs,
        jobs.len(),
        || (),
        |_, &value| {
            barrier.wait();
            for _ in 0..8 {
                crate::write_atomic(&destination, &vec![value; 16_384]).unwrap();
                let bytes = std::fs::read(&destination).unwrap();
                assert_eq!(bytes.len(), 16_384);
                assert!(bytes.iter().all(|byte| *byte == bytes[0]));
            }
        },
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}
