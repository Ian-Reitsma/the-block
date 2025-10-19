use foundation_serialization::json::{self, Map, Value};

use crate::scheduler::{self, ServiceClass};

pub fn stats() -> Value {
    let stats = scheduler::global_stats_snapshot();
    let mut queues = Map::new();
    for class in ServiceClass::ALL.iter() {
        let depth = stats.queue_depths.get(class).copied().unwrap_or(0);
        queues.insert(class.as_str().to_string(), Value::from(depth as u64));
    }
    let mut obj = Map::new();
    obj.insert(
        "reentrant_enabled".to_string(),
        Value::Bool(stats.reentrant_enabled),
    );
    obj.insert(
        "weights".to_string(),
        json::to_value(stats.weights.as_map()).unwrap_or(Value::Null),
    );
    obj.insert("queues".to_string(), Value::Object(queues));
    Value::Object(obj)
}
