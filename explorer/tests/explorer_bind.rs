use diagnostics::internal;
use std::io;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use the_block::net::listener;

#[test]
fn explorer_listener_conflict_logs_warning() {
    let holder = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("skipping explorer_listener_conflict_logs_warning: {err}");
            return;
        }
        Err(err) => panic!("bind holder: {err}"),
    };
    let addr = holder.local_addr().expect("listener addr");

    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let guard = internal::install_subscriber(move |record| {
        if record.target.as_ref() == "explorer" {
            if let Ok(mut logs) = sink.lock() {
                logs.push(record.message.to_string());
            }
        }
    });

    let result = runtime::block_on(async {
        listener::bind_runtime("explorer", "explorer_listener_bind_failed", addr).await
    });
    assert!(
        result.is_err(),
        "binding should fail while holder is active"
    );

    drop(guard);
    drop(holder);

    let logs = captured.lock().expect("capture");
    assert!(
        logs.iter()
            .any(|msg| msg.contains("explorer_listener_bind_failed")),
        "expected explorer bind warning, got {:?}",
        *logs
    );
}
