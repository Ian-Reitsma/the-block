#[cfg(feature = "quinn")]
mod rustls_store {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use rustls::client::ClientSessionStore;
    use rustls::internal::msgs::persist;
    use rustls::{NamedGroup, ServerName};

    const MAX_TLS13_TICKETS_PER_SERVER: usize = 8;

    #[derive(Clone)]
    pub struct SessionResumeStore {
        inner: Arc<Mutex<SessionState>>,
    }

    struct SessionState {
        max_servers: usize,
        servers: HashMap<ServerName, ServerCacheEntry>,
        order: VecDeque<ServerName>,
    }

    #[derive(Default)]
    struct ServerCacheEntry {
        kx_hint: Option<NamedGroup>,
        tls12: Option<persist::Tls12ClientSessionValue>,
        tls13: VecDeque<persist::Tls13ClientSessionValue>,
    }

    impl SessionResumeStore {
        pub fn new(max_entries: usize) -> Self {
            let max_servers = max_entries
                .max(1)
                .saturating_add(MAX_TLS13_TICKETS_PER_SERVER - 1)
                / MAX_TLS13_TICKETS_PER_SERVER;
            Self {
                inner: Arc::new(Mutex::new(SessionState {
                    max_servers: max_servers.max(1),
                    servers: HashMap::new(),
                    order: VecDeque::new(),
                })),
            }
        }

        pub fn clear(&self) {
            let mut state = self.inner.lock().unwrap();
            state.servers.clear();
            state.order.clear();
        }

        pub fn server_count(&self) -> usize {
            self.inner.lock().unwrap().servers.len()
        }

        fn with_entry<F>(&self, server_name: &ServerName, mut f: F)
        where
            F: FnMut(&mut ServerCacheEntry),
        {
            let mut state = self.inner.lock().unwrap();
            let entry = state.ensure_entry(server_name.clone());
            f(entry);
        }

        fn with_entry_mut<F, R>(&self, server_name: &ServerName, mut f: F) -> Option<R>
        where
            F: FnMut(&mut ServerCacheEntry) -> Option<R>,
        {
            let mut state = self.inner.lock().unwrap();
            if !state.servers.contains_key(server_name) {
                return None;
            }
            state.touch(server_name);
            let entry = state.servers.get_mut(server_name)?;
            f(entry)
        }
    }

    impl SessionState {
        fn ensure_entry(&mut self, server_name: ServerName) -> &mut ServerCacheEntry {
            if !self.servers.contains_key(&server_name) {
                self.servers
                    .insert(server_name.clone(), ServerCacheEntry::default());
                self.order.push_back(server_name.clone());
                self.trim();
            } else {
                self.touch(&server_name);
            }
            self.servers.get_mut(&server_name).unwrap()
        }

        fn touch(&mut self, server_name: &ServerName) {
            if let Some(position) = self.order.iter().position(|name| name == server_name) {
                if position + 1 != self.order.len() {
                    if let Some(item) = self.order.remove(position) {
                        self.order.push_back(item);
                    }
                }
            } else {
                self.order.push_back(server_name.clone());
                self.trim();
            }
        }

        fn trim(&mut self) {
            while self.servers.len() > self.max_servers {
                if let Some(evicted) = self.order.pop_front() {
                    self.servers.remove(&evicted);
                } else {
                    break;
                }
            }
        }
    }

    impl ClientSessionStore for SessionResumeStore {
        fn set_kx_hint(&self, server_name: &ServerName, group: NamedGroup) {
            self.with_entry(server_name, |entry| entry.kx_hint = Some(group));
        }

        fn kx_hint(&self, server_name: &ServerName) -> Option<NamedGroup> {
            self.inner
                .lock()
                .unwrap()
                .servers
                .get(server_name)
                .and_then(|entry| entry.kx_hint)
        }

        fn set_tls12_session(
            &self,
            server_name: &ServerName,
            value: persist::Tls12ClientSessionValue,
        ) {
            let mut slot = Some(value);
            self.with_entry(server_name, |entry| entry.tls12 = slot.take());
        }

        fn tls12_session(
            &self,
            server_name: &ServerName,
        ) -> Option<persist::Tls12ClientSessionValue> {
            self.inner
                .lock()
                .unwrap()
                .servers
                .get(server_name)
                .and_then(|entry| entry.tls12.as_ref().cloned())
        }

        fn remove_tls12_session(&self, server_name: &ServerName) {
            let mut state = self.inner.lock().unwrap();
            if let Some(entry) = state.servers.get_mut(server_name) {
                entry.tls12 = None;
            }
        }

        fn insert_tls13_ticket(
            &self,
            server_name: &ServerName,
            value: persist::Tls13ClientSessionValue,
        ) {
            let mut slot = Some(value);
            self.with_entry(server_name, |entry| {
                if entry.tls13.len() == MAX_TLS13_TICKETS_PER_SERVER {
                    entry.tls13.pop_front();
                }
                if let Some(value) = slot.take() {
                    entry.tls13.push_back(value);
                }
            });
        }

        fn take_tls13_ticket(
            &self,
            server_name: &ServerName,
        ) -> Option<persist::Tls13ClientSessionValue> {
            self.with_entry_mut(server_name, |entry| entry.tls13.pop_back())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn trim_removes_oldest_servers() {
            let store = SessionResumeStore::new(1);
            let server1 = ServerName::try_from("example.com").unwrap();
            let server2 = ServerName::try_from("example.net").unwrap();
            store.set_kx_hint(&server1, NamedGroup::secp256r1());
            store.set_kx_hint(&server2, NamedGroup::secp384r1());
            assert_eq!(store.server_count(), 1);
            assert!(store.kx_hint(&server1).is_none());
            assert!(store.kx_hint(&server2).is_some());
        }
    }

    pub use SessionResumeStore as Store;
}

#[cfg(not(feature = "quinn"))]
mod stub_store {
    #[derive(Clone, Default)]
    pub struct SessionResumeStore;

    impl SessionResumeStore {
        pub fn new(_max_entries: usize) -> Self {
            Self
        }

        pub fn clear(&self) {}

        pub fn server_count(&self) -> usize {
            0
        }
    }

    pub use SessionResumeStore as Store;
}

#[cfg(feature = "quinn")]
pub use rustls_store::Store as SessionResumeStore;
#[cfg(not(feature = "quinn"))]
pub use stub_store::Store as SessionResumeStore;
