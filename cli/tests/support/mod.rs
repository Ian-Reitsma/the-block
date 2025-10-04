pub mod json_rpc {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use httpd::{Response, Router, ServerConfig, StatusCode};
    use runtime::{self, net::TcpListener};

    #[derive(Clone)]
    struct JsonRpcState {
        responses: Arc<Mutex<VecDeque<String>>>,
        captured: Arc<Mutex<Vec<String>>>,
    }

    pub struct JsonRpcMock {
        url: String,
        state: JsonRpcState,
        handle: runtime::JoinHandle<std::io::Result<()>>,
    }

    impl JsonRpcMock {
        pub fn start(responses: Vec<String>) -> Self {
            runtime::block_on(async {
                let state = JsonRpcState {
                    responses: Arc::new(Mutex::new(VecDeque::from(responses))),
                    captured: Arc::new(Mutex::new(Vec::new())),
                };
                let router_state = state.clone();
                let router = Router::new(router_state).post("/", |req| {
                    let state = req.state().clone();
                    async move {
                        let body = req.take_body();
                        let body_text = String::from_utf8_lossy(&body).to_string();
                        if let Ok(mut captured) = state.captured.lock() {
                            captured.push(body_text);
                        }
                        let response_body = state
                            .responses
                            .lock()
                            .map(|mut queue| {
                                queue.pop_front().unwrap_or_else(|| {
                                    "{\"jsonrpc\":\"2.0\",\"result\":null,\"id\":1}".to_string()
                                })
                            })
                            .unwrap_or_else(|_| {
                                "{\"jsonrpc\":\"2.0\",\"result\":null,\"id\":1}".to_string()
                            });
                        Ok(Response::new(StatusCode::OK)
                            .with_header("content-type", "application/json")
                            .with_body(response_body.into_bytes()))
                    }
                });
                let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())
                    .await
                    .expect("bind test listener");
                let addr = format!("http://{}", listener.local_addr().expect("listener addr"));
                let handle = runtime::spawn(async move {
                    httpd::serve(listener, router, ServerConfig::default()).await
                });
                JsonRpcMock {
                    url: addr,
                    state,
                    handle,
                }
            })
        }

        pub fn url(&self) -> &str {
            &self.url
        }

        pub fn captured(&self) -> Vec<String> {
            self.state
                .captured
                .lock()
                .map(|data| data.clone())
                .unwrap_or_default()
        }
    }

    impl Drop for JsonRpcMock {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }
}
