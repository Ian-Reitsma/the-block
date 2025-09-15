use the_block::scheduler::{ServiceClass, ServiceScheduler, ServiceWeights};

/// Simulate a mixed gossip/compute/storage workload to validate fairness.
pub fn simulate_rounds(rounds: usize) -> Vec<ServiceClass> {
    let mut scheduler = ServiceScheduler::new(ServiceWeights::default());
    let mut history = Vec::new();
    for step in 0..rounds {
        scheduler.enqueue(ServiceClass::Gossip, format!("gossip-{step}"));
        if step % 2 == 0 {
            scheduler.enqueue(ServiceClass::Compute, format!("compute-{step}"));
        }
        if step % 3 == 0 {
            scheduler.enqueue(ServiceClass::Storage, format!("storage-{step}"));
        }
        if let Some(task) = scheduler.dequeue() {
            history.push(task.class);
        }
    }
    history
}
