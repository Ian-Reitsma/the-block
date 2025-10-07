use foundation_serialization::json::{json, Map, Value};

use crate::scheduler::{self, ServiceClass};

pub fn stats() -> Value {
    let stats = scheduler::global_stats_snapshot();
    let mut queues = Map::new();
    for class in ServiceClass::ALL.iter() {
        let depth = stats.queue_depths.get(class).copied().unwrap_or(0);
        queues.insert(class.as_str().to_string(), Value::from(depth as u64));
    }
    json!({
        "reentrant_enabled": stats.reentrant_enabled,
        "weights": stats.weights.as_map(),
        "queues": queues,
    })
}
