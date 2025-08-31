use rayon::prelude::*;
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
        let mut groups: Vec<Vec<Task<T>>> = Vec::new();
        for task in tasks {
            let mut task_opt = Some(task);
            for group in groups.iter_mut() {
                if let Some(ref t) = task_opt {
                    if !conflicts(t, group) {
                        group.push(task_opt.take().unwrap());
                        break;
                    }
                }
            }
            if let Some(t) = task_opt {
                groups.push(vec![t]);
            }
        }
        groups
            .into_iter()
            .flat_map(|g| g.into_par_iter().map(|t| (t.func)()).collect::<Vec<_>>())
            .collect()
    }
}
