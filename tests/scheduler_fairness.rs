use the_block::scheduler::{ServiceClass, ServiceScheduler, ServiceWeights};

#[test]
fn scheduler_honors_class_allocation() {
    let mut scheduler = ServiceScheduler::new(ServiceWeights::new(1, 2, 1));
    scheduler.enqueue(ServiceClass::Gossip, "g1");
    scheduler.enqueue(ServiceClass::Compute, "c1");
    scheduler.enqueue(ServiceClass::Compute, "c2");
    scheduler.enqueue(ServiceClass::Storage, "s1");
    scheduler.enqueue(ServiceClass::Compute, "c3");

    let mut order = Vec::new();
    while let Some(task) = scheduler.dequeue() {
        order.push((task.class, task.payload));
    }

    #[cfg(feature = "reentrant_scheduler")]
    {
        assert_eq!(order.len(), 5);
        assert_eq!(order[0].0, ServiceClass::Gossip);
        assert_eq!(order[1].0, ServiceClass::Compute);
        assert_eq!(order[2].0, ServiceClass::Compute);
        assert_eq!(order[3].0, ServiceClass::Storage);
        assert_eq!(order[4].0, ServiceClass::Compute);
    }

    #[cfg(not(feature = "reentrant_scheduler"))]
    {
        let expected = [
            ServiceClass::Gossip,
            ServiceClass::Compute,
            ServiceClass::Compute,
            ServiceClass::Storage,
            ServiceClass::Compute,
        ];
        assert_eq!(order.len(), expected.len());
        for (idx, (class, _)) in order.iter().enumerate() {
            assert_eq!(*class, expected[idx]);
        }
    }
}
