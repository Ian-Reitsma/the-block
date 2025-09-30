use runtime::net::TcpStream;
use runtime::ws::{self, Message as WsMessage, ServerStream};

#[cfg(feature = "telemetry")]
use crate::telemetry;
use crate::vm::{vm_debug_enabled, Debugger};

/// Perform WebSocket handshake and stream VM execution trace.
pub async fn serve_vm_trace(mut stream: TcpStream, key: String, code: Vec<u8>) {
    if !vm_debug_enabled() {
        let _ = stream.shutdown().await;
        return;
    }
    if ws::write_server_handshake(&mut stream, &key, &[])
        .await
        .is_err()
    {
        return;
    }
    let ws_stream = ServerStream::new(stream);
    #[cfg(feature = "telemetry")]
    telemetry::VM_TRACE_TOTAL.inc();
    run_trace(ws_stream, code).await;
}

async fn run_trace(mut ws: ServerStream, code: Vec<u8>) {
    let mut dbg = Debugger::new(code);
    let steps = dbg.run().to_vec();
    for step in steps {
        if ws
            .send(WsMessage::Text(serde_json::to_string(&step).unwrap()))
            .await
            .is_err()
        {
            break;
        }
    }
}
