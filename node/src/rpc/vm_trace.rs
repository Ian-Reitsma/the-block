use runtime::ws::{Message as WsMessage, ServerStream};

use crate::vm::{vm_debug_enabled, Debugger};

/// Stream VM execution trace over an upgraded WebSocket connection.
pub async fn run_trace(mut ws: ServerStream, code: Vec<u8>) {
    if !vm_debug_enabled() {
        return;
    }
    let mut dbg = Debugger::new(code);
    let steps = dbg.run().to_vec();
    for step in steps {
        if ws
            .send(WsMessage::Text(
                foundation_serialization::json::to_string(&step).unwrap(),
            ))
            .await
            .is_err()
        {
            break;
        }
    }
}
