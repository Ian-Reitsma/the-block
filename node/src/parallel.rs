use std::collections::HashSet;

/// A unit of work with explicit read/write sets.
pub struct Task<T: Send + Sync> {
    pub reads: HashSet<String>,
    pub writes: HashSet<String>,
    func: Box<dyn Fn() -> T + Send + Sync>,
}

impl<T: Send + Sync> Task<T> {
    /// Create a new task.
    pub fn new<F>(reads: Vec<String>, writes: Vec<String>, f: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Self {
            reads: reads.into_iter().collect(),
            writes: writes.into_iter().collect(),
            func: Box::new(f),
        }
    }
}

fn conflicts<T: Send + Sync>(task: &Task<T>, group: &[Task<T>]) -> bool {
    for g in group {
        if !task.writes.is_disjoint(&g.reads)
            || !task.reads.is_disjoint(&g.writes)
            || !task.writes.is_disjoint(&g.writes)
        {
            return true;
        }
    }
    false
}

/// Executor that schedules non-overlapping tasks in parallel.
pub struct ParallelExecutor;

impl ParallelExecutor {
    /// Execute tasks, partitioning them to avoid state conflicts.
    pub fn execute<T: Send + Sync>(tasks: Vec<Task<T>>) -> Vec<T> {
        #[cfg(feature = "telemetry")]
        let start = std::time::Instant::now();
        let mut groups: Vec<Vec<Task<T>>> = Vec::new();
        for task in tasks {
            let mut task_opt = Some(task);
            for group in groups.iter_mut() {
                if let Some(ref t) = task_opt {
                    if !conflicts(t, group) {
                        if let Some(task_inner) = task_opt.take() {
                            group.push(task_inner);
                        }
                        break;
                    }
                }
            }
            if let Some(t) = task_opt {
                groups.push(vec![t]);
            }
        }
        let total_tasks: usize = groups.iter().map(|g| g.len()).sum();
        let mut results = Vec::with_capacity(total_tasks);
        for mut group in groups {
            std::thread::scope(|scope| {
                let mut handles = Vec::with_capacity(group.len());
                for task in group.drain(..) {
                    handles.push(scope.spawn(move || (task.func)()));
                }
                for handle in handles {
                    results.push(handle.join().expect("parallel task panicked"));
                }
            });
        }
        #[cfg(feature = "telemetry")]
        {
            crate::telemetry::sampled_observe(
                &crate::telemetry::PARALLEL_EXECUTE_SECONDS,
                start.elapsed().as_secs_f64(),
            );
            crate::telemetry::update_memory_usage(crate::telemetry::MemoryComponent::Compute);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn executes_non_conflicting_in_parallel() {
        let t1 = Task::new(vec!["r1".into()], vec!["w1".into()], || {
            std::thread::sleep(Duration::from_millis(50));
        });
        let t2 = Task::new(vec!["r2".into()], vec!["w2".into()], || {
            std::thread::sleep(Duration::from_millis(50));
        });
        let start = Instant::now();
        ParallelExecutor::execute(vec![t1, t2]);
        assert!(start.elapsed() < Duration::from_millis(90));
    }

    #[test]
    fn serializes_conflicting_tasks() {
        let t1 = Task::new(vec!["r1".into()], vec!["w1".into()], || {
            std::thread::sleep(Duration::from_millis(50));
        });
        let t2 = Task::new(vec!["w1".into()], vec!["w2".into()], || {
            std::thread::sleep(Duration::from_millis(50));
        });
        let start = Instant::now();
        ParallelExecutor::execute(vec![t1, t2]);
        assert!(start.elapsed() >= Duration::from_millis(100));
    }
}
