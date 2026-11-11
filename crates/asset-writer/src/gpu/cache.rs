//! Bounded reuse shared by independent texture owners and concurrent cook jobs.
use anyhow::Result;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, OnceLock},
};

type Entry = Arc<OnceLock<Result<Vec<u8>, String>>>;
#[derive(Default)]
struct Cache {
    bytes: usize,
    entries: HashMap<[u8; 32], Entry>,
}
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::default()));
const LIMIT: usize = 64 * 1024 * 1024;

pub(super) fn get(
    key: [u8; 32],
    bytes: usize,
    encode: impl FnOnce() -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    if bytes > LIMIT {
        return encode();
    }
    let entry = {
        let mut cache = CACHE.lock().unwrap();
        if let Some(entry) = cache.entries.get(&key) {
            entry.clone()
        } else {
            if cache.bytes + bytes > LIMIT {
                cache.entries.clear();
                cache.bytes = 0;
            }
            let entry = Arc::new(OnceLock::new());
            cache.entries.insert(key, entry.clone());
            cache.bytes += bytes;
            entry
        }
    };
    entry
        .get_or_init(|| encode().map_err(|e| format!("{e:#}")))
        .as_ref()
        .cloned()
        .map_err(|e| anyhow::anyhow!(e.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_owners_share_one_encoding() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let jobs = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        get([219; 32], 1024, || {
                            calls.fetch_add(1, Ordering::Relaxed);
                            std::thread::sleep(std::time::Duration::from_millis(5));
                            Ok(vec![7; 16])
                        })
                        .unwrap()
                    })
                })
                .collect::<Vec<_>>();
            for job in jobs {
                assert_eq!(job.join().unwrap(), vec![7; 16]);
            }
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
