//! Shared task ownership and retained results. Hosts keep their own scheduling.
use std::collections::{BTreeMap, BTreeSet};

const RETAINED_TASK_LIMIT: usize = 256;

struct Child {
    parent: i32,
    result: Option<Vec<i32>>,
}

/// Ownership/results only: each host retains its existing execution order.
#[derive(Default)]
pub struct Tasks {
    children: BTreeMap<i32, Child>,
    released: BTreeSet<i32>,
}

impl Tasks {
    pub fn register(&mut self, parent: i32, handle: i32) -> Result<(), String> {
        if self.children.len() >= RETAINED_TASK_LIMIT {
            return Err("too many unjoined child tasks".into());
        }
        if handle == parent || self.root(parent) == handle {
            return Err("cyclic child task ownership".into());
        }
        if self.children.contains_key(&handle) {
            return Err("duplicate child task handle".into());
        }
        self.children.insert(
            handle,
            Child {
                parent,
                result: None,
            },
        );
        Ok(())
    }
    pub fn join(&mut self, parent: i32, handle: i32) -> Result<Option<Vec<i32>>, String> {
        let child = self
            .children
            .get(&handle)
            .ok_or("child task handle is stale or already joined")?;
        if child.parent != parent {
            return Err("task belongs to a different parent".into());
        }
        if child.result.is_none() {
            return Ok(None);
        }
        Ok(self.children.remove(&handle).unwrap().result)
    }
    pub fn children(&self, parent: i32) -> Vec<i32> {
        self.children
            .iter()
            .filter_map(|(&handle, child)| (child.parent == parent).then_some(handle))
            .collect()
    }
    pub fn finish(&mut self, handle: i32, result: Vec<i32>) {
        self.released.remove(&handle);
        if let Some(child) = self.children.get_mut(&handle) {
            child.result = Some(result);
        }
    }
    pub fn root(&self, mut handle: i32) -> i32 {
        while let Some(child) = self.children.get(&handle) {
            handle = child.parent;
        }
        handle
    }
    pub fn contains(&self, handle: i32) -> bool {
        self.children.contains_key(&handle)
    }
    pub fn remove(&mut self, handle: i32) {
        self.released.remove(&handle);
        self.children.remove(&handle);
    }
    pub fn release_control(&mut self, handle: i32) {
        self.released.insert(self.root(handle));
    }
    pub fn control_released(&self, handle: i32) -> bool {
        self.released.contains(&self.root(handle))
    }
    pub fn clear(&mut self) {
        self.released.clear();
        self.children.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_rejects_cycles_foreign_joins_and_consumed_results() {
        let mut tasks = Tasks::default();
        tasks.register(1, 2).unwrap();
        tasks.register(2, 3).unwrap();
        assert!(tasks.register(3, 1).is_err());
        assert!(tasks.register(4, 4).is_err());
        assert_eq!(tasks.root(3), 1);
        assert!(tasks.join(1, 3).is_err());
        assert_eq!(tasks.join(2, 3).unwrap(), None);
        tasks.finish(3, vec![42]);
        assert_eq!(tasks.join(2, 3).unwrap(), Some(vec![42]));
        assert!(tasks.join(2, 3).is_err());
    }
}
