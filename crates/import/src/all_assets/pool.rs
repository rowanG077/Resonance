//! Bounded workers retain decoder state; dependency completion unlocks queued jobs.
use anyhow::{Context, Result, anyhow, ensure};
use std::{
    any::Any,
    collections::{HashSet, VecDeque},
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

struct Finished {
    worker: Option<usize>,
    index: usize,
    result: Result<Value>,
}

fn copy_result<T: ?Sized>(result: &Result<Arc<T>>) -> Result<Arc<T>> {
    result
        .as_ref()
        .map(Arc::clone)
        .map_err(|error| anyhow!("{error:#}"))
}

#[derive(Clone, Copy)]
struct Estimate {
    output: usize,
    scratch: usize,
}

impl Estimate {
    fn reservation(self) -> usize {
        // Construction validates overflow before any worker starts.
        self.output + self.scratch
    }
}

/// Accounts for declared working sets, not process RSS or deliberately retained receipts.
struct MemoryBudget<'a> {
    limit: usize,
    used: usize,
    estimates: &'a [Estimate],
    running: Vec<bool>,
    resident: Vec<bool>,
    readers: Vec<usize>,
}

impl<'a> MemoryBudget<'a> {
    fn new<S>(
        limit: usize,
        estimates: &'a [Estimate],
        jobs: &[ComputationJob<'_, S>],
    ) -> Result<Self> {
        ensure!(limit > 0, "asset memory budget must be positive");
        for (index, job) in jobs.iter().enumerate() {
            let estimate = estimates[index];
            let required = job
                .dependencies
                .iter()
                .fold(
                    estimate.output.checked_add(estimate.scratch),
                    |total, &parent| {
                        total.and_then(|size| size.checked_add(estimates[parent.index].output))
                    },
                )
                .context("asset working-set estimate overflows")?;
            ensure!(
                required <= limit,
                "asset job {index} working set needs {required} bytes (inputs, output and scratch), memory limit {limit} bytes"
            );
        }
        Ok(Self {
            limit,
            used: 0,
            estimates,
            running: vec![false; jobs.len()],
            resident: vec![false; jobs.len()],
            readers: vec![0; jobs.len()],
        })
    }

    fn fits(&self, index: usize) -> bool {
        self.estimates[index].reservation() <= self.limit - self.used
    }

    fn start(&mut self, index: usize, dependencies: &[Dependency]) {
        self.used += self.estimates[index].reservation();
        self.running[index] = true;
        self.resident[index] = true;
        for parent in dependencies {
            self.readers[parent.index] += 1;
        }
    }

    fn release(&mut self, index: usize, consumers: usize) {
        if self.resident[index]
            && !self.running[index]
            && self.readers[index] == 0
            && consumers == 0
        {
            self.used -= self.estimates[index].output;
            self.resident[index] = false;
        }
    }

    fn finish(
        &mut self,
        index: usize,
        dependencies: &[Dependency],
        consumers: &[usize],
        succeeded: bool,
    ) {
        self.running[index] = false;
        self.used -= self.estimates[index].scratch;
        self.release(index, if succeeded { consumers[index] } else { 0 });
        for parent in dependencies {
            self.readers[parent.index] -= 1;
            self.release(parent.index, consumers[parent.index]);
        }
    }
}

type Value = Arc<dyn Any + Send + Sync>;

/// A typed edge; handles are valid only in the graph that created them.
pub(crate) struct Output<T> {
    dependency: Dependency,
    marker: PhantomData<fn() -> T>,
}

impl<T> Copy for Output<T> {}
impl<T> Clone for Output<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Output<T> {
    pub(crate) fn dependency(self) -> Dependency {
        self.dependency
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dependency {
    graph: u64,
    index: usize,
}

/// Provides only the current job's declared inputs, never a global asset registry.
pub(crate) struct Resolver<'a> {
    declarations: &'a [Dependency],
    values: &'a [Value],
}

impl Resolver<'_> {
    pub(crate) fn get<T: Any + Send + Sync>(&self, output: Output<T>) -> Result<Arc<T>> {
        let position = self
            .declarations
            .iter()
            .position(|&input| input == output.dependency)
            .context("asset job requested an undeclared dependency")?;
        Arc::clone(&self.values[position])
            .downcast()
            .map_err(|_| anyhow!("asset dependency type mismatch"))
    }
}

type Computation<'a, S> = dyn Fn(&mut S, &Resolver<'_>) -> Result<Value> + Sync + 'a;

struct ComputationJob<'a, S> {
    name: String,
    estimate: Option<Estimate>,
    dependencies: Vec<Dependency>,
    execute: Box<Computation<'a, S>>,
}

/// A coordinator-side completion view. Retain final receipts, not decoded inputs.
pub(crate) struct Completion<'a> {
    pub(crate) name: &'a str,
    output: Dependency,
    result: &'a Result<Value>,
}

impl Completion<'_> {
    pub(crate) fn error(&self) -> Option<&anyhow::Error> {
        self.result.as_ref().err()
    }

    /// Homogeneous graphs can inspect a result without scanning every output handle.
    pub(crate) fn result<T: Any + Send + Sync>(&self) -> Result<Arc<T>> {
        copy_result(self.result)?
            .downcast()
            .map_err(|_| anyhow!("asset completion type mismatch"))
    }

    /// None means this completion belongs to another output.
    pub(crate) fn get<T: Any + Send + Sync>(&self, output: Output<T>) -> Option<Result<Arc<T>>> {
        (self.output == output.dependency).then(|| self.result())
    }
}

/// Heterogeneous in-memory jobs on one bounded scheduler.
/// Outputs own their data; computations may borrow the caller's source context.
pub(crate) struct Dag<'a, S> {
    id: u64,
    jobs: Vec<ComputationJob<'a, S>>,
}

impl<'a, S: 'a> Dag<'a, S> {
    pub(crate) fn new() -> Self {
        static NEXT_GRAPH: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_GRAPH.fetch_add(1, Ordering::Relaxed),
            jobs: Vec::new(),
        }
    }

    pub(crate) fn add<T: Any + Send + Sync>(
        &mut self,
        name: impl Into<String>,
        dependencies: impl IntoIterator<Item = Dependency>,
        execute: impl Fn(&mut S, &Resolver<'_>) -> Result<T> + Sync + 'a,
    ) -> Output<T> {
        self.add_shared(name, dependencies, move |state, inputs| {
            execute(state, inputs).map(Arc::new)
        })
    }

    /// A COW transformation can return its modified Arc directly.
    pub(crate) fn add_shared<T: Any + Send + Sync>(
        &mut self,
        name: impl Into<String>,
        dependencies: impl IntoIterator<Item = Dependency>,
        execute: impl Fn(&mut S, &Resolver<'_>) -> Result<Arc<T>> + Sync + 'a,
    ) -> Output<T> {
        let output = Output {
            dependency: Dependency {
                graph: self.id,
                index: self.jobs.len(),
            },
            marker: PhantomData,
        };
        self.jobs.push(ComputationJob {
            name: name.into(),
            estimate: None,
            dependencies: dependencies.into_iter().collect(),
            execute: Box::new(move |state, inputs| Ok(execute(state, inputs)?)),
        });
        output
    }

    /// Receipts are observed on the coordinator; returned statuses contain no payloads.
    pub(crate) fn run(
        self,
        workers: usize,
        init: impl Fn() -> S + Sync,
        complete: impl FnMut(Completion<'_>),
    ) -> Result<Vec<Result<()>>> {
        self.execute(workers, None, init, complete)
    }

    /// Count all reachable output buffers (even shared ones) and temporary allocations.
    /// Dependency bytes are retained separately until executing consumers release them.
    pub(crate) fn estimate<T>(
        &mut self,
        output: Output<T>,
        output_bytes: usize,
        scratch_bytes: usize,
    ) -> Result<()> {
        ensure!(
            output.dependency.graph == self.id,
            "memory estimate belongs to another asset graph"
        );
        self.jobs[output.dependency.index].estimate = Some(Estimate {
            output: output_bytes,
            scratch: scratch_bytes,
        });
        Ok(())
    }

    /// Every job needs an estimate. Impossible working sets fail rather than exceeding the limit.
    pub(crate) fn run_bounded(
        self,
        workers: usize,
        budget_bytes: usize,
        init: impl Fn() -> S + Sync,
        complete: impl FnMut(Completion<'_>),
    ) -> Result<Vec<Result<()>>> {
        self.execute(workers, Some(budget_bytes), init, complete)
    }

    fn execute(
        self,
        workers: usize,
        budget: Option<usize>,
        init: impl Fn() -> S + Sync,
        mut complete: impl FnMut(Completion<'_>),
    ) -> Result<Vec<Result<()>>> {
        let jobs = &self.jobs;
        ensure!(workers > 0, "asset worker count must be positive");
        let mut children = vec![Vec::new(); jobs.len()];
        let mut remaining = Vec::with_capacity(jobs.len());
        for (index, job) in jobs.iter().enumerate() {
            let mut seen = HashSet::new();
            for &parent in &job.dependencies {
                ensure!(
                    parent.graph == self.id,
                    "asset job '{}': dependency belongs to another graph",
                    job.name
                );
                ensure!(
                    parent.index < index,
                    "asset job {index}: dependency {} must precede its consumer",
                    parent.index
                );
                ensure!(
                    seen.insert(parent.index),
                    "asset job {index}: duplicate dependency {}",
                    parent.index
                );
                children[parent.index].push(index);
            }
            remaining.push(job.dependencies.len());
        }
        let estimates = budget
            .map(|_| {
                jobs.iter()
                    .map(|job| {
                        job.estimate.with_context(|| {
                            format!("asset job '{}' needs a memory estimate", job.name)
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let mut ready: VecDeque<_> = (0..jobs.len())
            .filter(|&index| remaining[index] == 0)
            .collect();
        let mut consumers = children.iter().map(Vec::len).collect::<Vec<_>>();
        let mut memory = budget
            .zip(estimates.as_deref())
            .map(|(limit, estimates)| MemoryBudget::new(limit, estimates, jobs))
            .transpose()?;

        thread::scope(|scope| {
            let (completed, events) = mpsc::channel::<Result<Finished>>();
            let mut senders = Vec::new();
            let mut handles = Vec::new();
            for worker in 0..workers.min(jobs.len()) {
                let (sender, work) = mpsc::channel::<(usize, Vec<Value>)>();
                senders.push(sender);
                let completed = completed.clone();
                let init = &init;
                let publisher = crate::publication::current();
                handles.push(scope.spawn(move || {
                    let _publisher = crate::publication::inherit(publisher);
                    let outcome = catch_unwind(AssertUnwindSafe(|| {
                        let mut state = init();
                        while let Ok((index, dependencies)) = work.recv() {
                            let job = &jobs[index];
                            let result = (job.execute)(
                                &mut state,
                                &Resolver {
                                    declarations: &job.dependencies,
                                    values: &dependencies,
                                },
                            )
                            .with_context(|| format!("asset job '{}'", job.name));
                            drop(dependencies);
                            if completed
                                .send(Ok(Finished {
                                    worker: Some(worker),
                                    index,
                                    result,
                                }))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }));
                    if let Err(panic) = outcome {
                        let message = panic
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| panic.downcast_ref::<&str>().copied())
                            .unwrap_or("unknown panic");
                        let error = format!("asset worker {worker} panicked: {message}");
                        let _ = completed.send(Err(anyhow!("{error}")));
                        return Err(anyhow!(error));
                    }
                    Ok(())
                }));
            }
            let mut idle = (0..senders.len()).rev().collect::<Vec<_>>();
            let mut values: Vec<Option<Value>> = (0..jobs.len()).map(|_| None).collect();
            let mut statuses: Vec<Option<Result<()>>> = (0..jobs.len()).map(|_| None).collect();
            let mut failed_parent: Vec<Option<usize>> = vec![None; jobs.len()];
            let mut pending_events = 0;
            for _ in 0..jobs.len() {
                while let Some(position) = ready.iter().position(|&index| {
                    failed_parent[index].is_some()
                        || (!idle.is_empty() && memory.as_ref().is_none_or(|m| m.fits(index)))
                }) {
                    let index = ready.remove(position).unwrap();
                    let mut dependencies = Vec::with_capacity(jobs[index].dependencies.len());
                    let mut blocked = failed_parent[index].map(|parent| {
                        let error = statuses[parent].as_ref().unwrap().as_ref().unwrap_err();
                        anyhow!("asset job {index}: dependency {parent} failed: {error:#}")
                    });
                    if blocked.is_none()
                        && let Some(memory) = &mut memory
                    {
                        memory.start(index, &jobs[index].dependencies);
                    }
                    // Consume every edge even if another parent failed, so skipped jobs do
                    // not keep successful siblings alive until the end of the cook.
                    for dependency in &jobs[index].dependencies {
                        let parent = dependency.index;
                        consumers[parent] -= 1;
                        if blocked.is_some() {
                            if consumers[parent] == 0 {
                                values[parent] = None;
                            }
                            if let Some(memory) = &mut memory {
                                memory.release(parent, consumers[parent]);
                            }
                            continue;
                        }
                        match statuses[parent].as_ref().expect("dependency completed") {
                            Ok(()) => dependencies.push(if consumers[parent] == 0 {
                                values[parent].take().expect("dependency retained")
                            } else {
                                Arc::clone(values[parent].as_ref().expect("dependency retained"))
                            }),
                            Err(error) if blocked.is_none() => {
                                blocked = Some(anyhow!(
                                    "asset job {index}: dependency {parent} failed: {error:#}"
                                ))
                            }
                            Err(_) => (),
                        }
                    }
                    let dependencies = match blocked {
                        Some(error) => Err(error),
                        None => Ok(dependencies),
                    };
                    pending_events += 1;
                    match dependencies {
                        Ok(dependencies) => {
                            let worker = idle.pop().unwrap();
                            if senders[worker].send((index, dependencies)).is_err() {
                                break; // The worker's panic diagnostic is on the completion channel.
                            }
                        }
                        Err(error) => completed
                            .send(Ok(Finished {
                                worker: None,
                                index,
                                result: Err(error),
                            }))
                            .map_err(|_| anyhow!("asset completion receiver stopped"))?,
                    }
                }
                if pending_events == 0
                    && let Some(memory) = &memory
                {
                    let index = *ready.front().context("asset graph has no ready work")?;
                    anyhow::bail!(
                        "asset memory budget exhausted: {} bytes retained, ready job {index} needs {} additional bytes, limit {} bytes",
                        memory.used,
                        memory.estimates[index].reservation(),
                        memory.limit
                    );
                }
                let Finished {
                    worker,
                    index,
                    result,
                } = events.recv().context("asset workers stopped")??;
                pending_events -= 1;
                complete(Completion {
                    name: &jobs[index].name,
                    output: Dependency {
                        graph: self.id,
                        index,
                    },
                    result: &result,
                });
                if worker.is_some()
                    && let Some(memory) = &mut memory
                {
                    memory.finish(index, &jobs[index].dependencies, &consumers, result.is_ok());
                }
                statuses[index] = Some(match result {
                    Ok(value) => {
                        if consumers[index] > 0 {
                            values[index] = Some(value);
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                });
                if let Some(worker) = worker {
                    idle.push(worker);
                }
                let failed = statuses[index].as_ref().unwrap().is_err();
                for &child in children[index].iter().rev() {
                    remaining[child] -= 1;
                    if failed_parent[child].is_some() {
                        continue;
                    }
                    if failed {
                        failed_parent[child] = Some(index);
                        ready.push_front(child);
                    } else if remaining[child] == 0 {
                        ready.push_front(child);
                    }
                }
            }
            drop(senders);
            for handle in handles {
                handle
                    .join()
                    .map_err(|_| anyhow!("asset worker panic escaped recovery"))??;
            }
            Ok(statuses
                .into_iter()
                .map(|result| result.expect("every job completed"))
                .collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Barrier, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    #[test]
    fn memory_admission_counts_retained_outputs_and_inputs_still_used_by_workers() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let work = || {
            peak.fetch_max(active.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(5));
            active.fetch_sub(1, Ordering::SeqCst);
        };
        let mut dag = Dag::new();
        for name in ["first", "second"] {
            let source = dag.add(name, [], |_, _| {
                work();
                Ok(vec![0_u8; 32])
            });
            dag.estimate(source, 32, 32).unwrap();
            let consumer = dag.add(
                format!("consume {name}"),
                [source.dependency()],
                move |_, inputs| {
                    assert_eq!(inputs.get(source)?.len(), 32);
                    work();
                    Ok(())
                },
            );
            dag.estimate(consumer, 0, 24).unwrap();
        }
        let mut order = Vec::new();
        let statuses = dag
            .run_bounded(
                2,
                96,
                || (),
                |completion| order.push(completion.name.to_owned()),
            )
            .unwrap();
        assert!(statuses.iter().all(Result::is_ok));
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert_eq!(
            order,
            ["first", "consume first", "second", "consume second"]
        );
    }

    #[test]
    fn retained_inputs_that_exhaust_the_budget_fail_without_stranding_workers_or_values() {
        struct Tracked(Arc<AtomicUsize>);
        impl Drop for Tracked {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let values = Arc::new(AtomicUsize::new(0));
        let states = Arc::new(AtomicUsize::new(0));
        let mut dag = Dag::new();
        let a = dag.add("a", [], |_, _| Ok(Tracked(Arc::clone(&values))));
        let b = dag.add("b", [], |_, _| Ok(Tracked(Arc::clone(&values))));
        let c = dag.add("c", [], |_, _| Ok(Tracked(Arc::clone(&values))));
        for source in [a, b, c] {
            dag.estimate(source, 40, 0).unwrap();
        }
        for source in [a, b] {
            let consumer = dag.add("join", [source.dependency(), c.dependency()], |_, _| Ok(()));
            dag.estimate(consumer, 0, 0).unwrap();
        }
        let error = dag
            .run_bounded(2, 100, || Tracked(Arc::clone(&states)), |_| {})
            .unwrap_err();
        let error = error.to_string();
        assert!(error.contains("80 bytes retained"), "{error}");
        assert!(error.contains("40 additional bytes"), "{error}");
        assert!(error.contains("limit 100 bytes"), "{error}");
        assert_eq!(values.load(Ordering::SeqCst), 2);
        assert_eq!(states.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn missing_and_oversized_memory_estimates_fail_before_workers_start() {
        for estimate in [None, Some((128, 0)), Some((usize::MAX, 1))] {
            let mut dag = Dag::<()>::new();
            let output = dag.add("source", [], |_, _| Ok(()));
            if let Some((output_bytes, scratch)) = estimate {
                dag.estimate(output, output_bytes, scratch).unwrap();
            }
            let error = dag
                .run_bounded(1, 64, || panic!("invalid graph started a worker"), |_| {})
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(match estimate {
                    None => "needs a memory estimate",
                    Some((128, _)) => "128 bytes",
                    _ => "overflows",
                }),
                "{error}"
            );
        }
    }

    #[test]
    fn typed_branches_share_buffers_and_release_intermediates_before_unrelated_work() {
        #[derive(Clone)]
        struct Model {
            name: &'static str,
            vertices: Arc<Vec<u8>>,
        }
        let decoded = AtomicUsize::new(0);
        let mut dag = Dag::new();
        let source = dag.add("source", [], |_, _| {
            decoded.fetch_add(1, Ordering::SeqCst);
            Ok(Model {
                name: "original",
                vertices: Arc::new(vec![1, 2, 3]),
            })
        });
        let changed = dag.add_shared("changed", [source.dependency()], move |_, inputs| {
            let mut model = inputs.get(source)?;
            let vertices = Arc::clone(&model.vertices);
            Arc::make_mut(&mut model).name = "changed";
            assert!(Arc::ptr_eq(&vertices, &model.vertices));
            Ok(model)
        });
        let receipt = dag.add("publish", [changed.dependency()], move |_, inputs| {
            let model = inputs.get(changed)?;
            Ok((model.name, model.vertices.len()))
        });
        dag.add("unchanged", [source.dependency()], move |_, inputs| {
            assert_eq!(inputs.get(source)?.name, "original");
            Ok(())
        });
        dag.add("unrelated", [], |_, _| Ok(()));
        let mut source_lifetime = std::sync::Weak::<Model>::new();
        let mut changed_lifetime = std::sync::Weak::<Model>::new();
        let mut published = None;
        let mut order = Vec::new();
        let statuses = dag
            .run(
                1,
                || (),
                |completion| {
                    order.push(completion.name.to_owned());
                    if let Some(value) = completion.get(source) {
                        source_lifetime = Arc::downgrade(&value.unwrap());
                    }
                    if let Some(value) = completion.get(changed) {
                        changed_lifetime = Arc::downgrade(&value.unwrap());
                    }
                    if let Some(value) = completion.get(receipt) {
                        published = Some(value.unwrap());
                    }
                    if completion.name == "unrelated" {
                        assert!(source_lifetime.upgrade().is_none());
                        assert!(changed_lifetime.upgrade().is_none());
                    }
                },
            )
            .unwrap();
        assert!(statuses.iter().all(Result::is_ok));
        assert_eq!(decoded.load(Ordering::SeqCst), 1);
        assert_eq!(*published.unwrap(), ("changed", 3));
        assert_eq!(
            order,
            ["source", "changed", "publish", "unchanged", "unrelated"]
        );
    }

    #[test]
    fn failed_dependents_release_all_inputs_including_those_after_the_failure() {
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let mut dag = Dag::new();
        let source = dag.add("source", [], |_, _| Ok(vec![0_u8; 64]));
        let delayed = dag.add("delayed", [], |_, _| {
            wait.lock().unwrap().recv_timeout(Duration::from_secs(5))?;
            Ok(())
        });
        let broken = dag.add::<()>("broken", [], |_, _| Err(anyhow!("invalid source")));
        let skipped = dag.add::<()>(
            "skipped",
            [
                broken.dependency(),
                source.dependency(),
                delayed.dependency(),
            ],
            |_, _| panic!("dependent of a failed job must not execute"),
        );
        dag.add::<()>("descendant", [skipped.dependency()], |_, _| {
            panic!("failed ancestor reached a descendant")
        });
        dag.add("independent", [], |_, _| Ok(()));
        let mut lifetime = std::sync::Weak::<Vec<u8>>::new();
        let statuses = dag
            .run(
                2,
                || (),
                |completion| {
                    if let Some(value) = completion.get(source) {
                        lifetime = Arc::downgrade(&value.unwrap());
                    }
                    if completion.name == "skipped" {
                        assert!(lifetime.upgrade().is_none());
                        release.send(()).unwrap();
                    }
                },
            )
            .unwrap();
        assert!(statuses[0].is_ok());
        assert!(statuses[1].is_ok());
        assert!(statuses[2].is_err());
        assert!(statuses[3].is_err());
        assert!(statuses[4].is_err());
        assert!(statuses[5].is_ok());
    }

    #[test]
    fn resolvers_reject_undeclared_inputs_and_graphs_reject_foreign_handles() {
        let mut dag = Dag::new();
        let source = dag.add("source", [], |_, _| Ok(7));
        dag.add("undeclared", [], move |_, inputs| Ok(*inputs.get(source)?));
        let statuses = dag.run(1, || (), |_| {}).unwrap();
        assert!(
            format!("{:#}", statuses[1].as_ref().unwrap_err()).contains("undeclared dependency")
        );

        let mut other = Dag::<()>::new();
        other.add("foreign", [source.dependency()], |_, _| Ok(()));
        let error = other
            .run(1, || panic!("invalid graph must not start workers"), |_| {})
            .unwrap_err();
        assert!(error.to_string().contains("another graph"));
    }

    #[test]
    fn dependency_completion_unlocks_children_without_waiting_for_other_roots() {
        let roots = Barrier::new(2);
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let initialized = AtomicUsize::new(0);
        let reused = AtomicBool::new(false);
        let called = |calls: &mut usize| {
            *calls += 1;
            reused.fetch_or(*calls > 1, Ordering::SeqCst);
        };
        let mut dag = Dag::new();
        let slow = dag.add("slow", [], |state, _| {
            called(state);
            roots.wait();
            wait.lock().unwrap().recv_timeout(Duration::from_secs(5))?;
            Ok(100)
        });
        let fast = dag.add("fast", [], |state, _| {
            called(state);
            roots.wait();
            Ok(10)
        });
        let child = dag.add("child", [fast.dependency()], |state, inputs| {
            called(state);
            assert_eq!(*inputs.get(fast)?, 10);
            release.send(())?;
            Ok(20)
        });
        let joined = dag.add(
            "joined",
            [child.dependency(), slow.dependency(), fast.dependency()],
            |state, inputs| {
                called(state);
                Ok(*inputs.get(child)? + *inputs.get(slow)? + *inputs.get(fast)?)
            },
        );
        let mut sum = None;
        let statuses = dag
            .run(
                2,
                || {
                    initialized.fetch_add(1, Ordering::SeqCst);
                    0
                },
                |completion| {
                    if let Some(value) = completion.get(joined) {
                        sum = Some(*value.unwrap());
                        assert!(completion.result::<String>().is_err());
                    }
                },
            )
            .unwrap();
        assert!(statuses.iter().all(Result::is_ok));
        assert_eq!(sum, Some(130));
        assert_eq!(initialized.load(Ordering::SeqCst), 2);
        assert!(reused.load(Ordering::SeqCst));
    }

    #[test]
    fn invalid_graphs_never_start_workers_and_worker_count_is_bounded() {
        let initialized = AtomicUsize::new(0);
        let init = || {
            initialized.fetch_add(1, Ordering::SeqCst);
        };
        let mut dag = Dag::new();
        let parent = dag.add("parent", [], |_, _| Ok(()));
        dag.add(
            "duplicate",
            [parent.dependency(), parent.dependency()],
            |_, _| Ok(()),
        );
        let error = dag.run(2, init, |_| {}).unwrap_err();
        assert!(error.to_string().contains("duplicate dependency"));
        let single = || {
            let mut dag = Dag::new();
            dag.add("single", [], |_, _| Ok(()));
            dag
        };
        assert!(single().run(0, init, |_| {}).is_err());
        assert!(Dag::new().run(8, init, |_| {}).unwrap().is_empty());
        assert_eq!(initialized.load(Ordering::SeqCst), 0);
        single().run(24, init, |_| {}).unwrap();
        assert_eq!(initialized.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn worker_panics_abort_the_graph_without_stranding_dependents() {
        for initialization in [false, true] {
            let mut dag = Dag::new();
            let source = dag.add::<()>("panic", [], |_, _| panic!("decoder job failed"));
            dag.add("dependent", [source.dependency()], |_, _| Ok(()));
            let error = dag
                .run(
                    2,
                    || {
                        assert!(!initialization, "decoder initialization failed");
                    },
                    |_| {},
                )
                .unwrap_err();
            assert!(error.to_string().contains(if initialization {
                "decoder initialization failed"
            } else {
                "decoder job failed"
            }));
        }
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("decoder shutdown failed");
            }
        }
        let mut dag = Dag::new();
        dag.add("single", [], |_, _| Ok(()));
        let error = dag.run(1, || PanicOnDrop, |_| {}).unwrap_err();
        assert!(error.to_string().contains("decoder shutdown failed"));
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
    let mut dag = Dag::new();
    for value in &jobs {
        let (barrier, destination) = (&barrier, &destination);
        dag.add(value.to_string(), [], move |_, _| {
            barrier.wait();
            for _ in 0..8 {
                crate::write_atomic(destination, &vec![*value; 16_384]).unwrap();
                let bytes = std::fs::read(destination).unwrap();
                assert_eq!(bytes.len(), 16_384);
                assert!(bytes.iter().all(|byte| *byte == bytes[0]));
            }
            Ok(())
        });
    }
    for result in dag.run(jobs.len(), || (), |_| {}).unwrap() {
        result.unwrap();
    }
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}
