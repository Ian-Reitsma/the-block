use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Lightweight in-house session resume cache that mirrors the subset of the
/// rustls client store interface consumed by the transport crate. We retain the
/// ability to bound stored entries per server so higher layers can continue to
/// inspect cache pressure, but the store itself is now entirely first-party.
#[derive(Clone, Default)]
pub struct SessionResumeStore {
    inner: Arc<Mutex<SessionState>>,
}

#[derive(Default)]
struct SessionState {
    max_servers: usize,
    order: VecDeque<String>,
    entries: HashMap<String, ServerEntry>,
}

#[derive(Default)]
struct ServerEntry {
    tickets: VecDeque<Vec<u8>>,
    max_tickets: usize,
}

impl SessionResumeStore {
    pub fn new(max_entries: usize) -> Self {
        let max_servers = max_entries.max(1);
        SessionResumeStore {
            inner: Arc::new(Mutex::new(SessionState {
                max_servers,
                order: VecDeque::new(),
                entries: HashMap::new(),
            })),
        }
    }

    pub fn clear(&self) {
        let mut state = self.inner.lock().unwrap();
        state.entries.clear();
        state.order.clear();
    }

    pub fn server_count(&self) -> usize {
        self.inner.lock().unwrap().entries.len()
    }

    /// Records a session ticket for the provided server identifier. Older
    /// tickets are evicted once the per-server quota has been reached.
    pub fn record_ticket(&self, server: &str, ticket: Vec<u8>, max_tickets: usize) {
        let mut state = self.inner.lock().unwrap();
        let entry = state.ensure_entry(server.to_owned(), max_tickets.max(1));
        if entry.tickets.len() == entry.max_tickets {
            entry.tickets.pop_front();
        }
        entry.tickets.push_back(ticket);
    }

    /// Attempts to take the newest ticket recorded for the server.
    pub fn take_ticket(&self, server: &str) -> Option<Vec<u8>> {
        let mut state = self.inner.lock().unwrap();
        if !state.entries.contains_key(server) {
            return None;
        }
        let ticket = {
            let entry = state
                .entries
                .get_mut(server)
                .expect("server entry exists after contains_key check");
            entry.tickets.pop_back()
        };
        state.touch(server);
        ticket
    }
}

impl SessionState {
    fn ensure_entry(&mut self, server: String, max_tickets: usize) -> &mut ServerEntry {
        if !self.entries.contains_key(&server) {
            self.entries.insert(
                server.clone(),
                ServerEntry {
                    tickets: VecDeque::new(),
                    max_tickets,
                },
            );
            self.order.push_back(server.clone());
            self.trim();
        } else {
            self.touch(&server);
        }
        self.entries.get_mut(&server).expect("entry just inserted")
    }

    fn touch(&mut self, server: &str) {
        if let Some(pos) = self.order.iter().position(|item| item == server) {
            if pos + 1 != self.order.len() {
                if let Some(item) = self.order.remove(pos) {
                    self.order.push_back(item);
                }
            }
        } else {
            self.order.push_back(server.to_owned());
            self.trim();
        }
    }

    fn trim(&mut self) {
        while self.entries.len() > self.max_servers {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_old_servers() {
        let store = SessionResumeStore::new(1);
        store.record_ticket("example.com", vec![1], 2);
        store.record_ticket("example.net", vec![2], 2);
        assert_eq!(store.server_count(), 1);
        assert!(store.take_ticket("example.com").is_none());
        assert_eq!(store.take_ticket("example.net"), Some(vec![2]));
    }

    #[test]
    fn bounds_ticket_queue() {
        let store = SessionResumeStore::new(2);
        store.record_ticket("example.com", vec![1], 2);
        store.record_ticket("example.com", vec![2], 2);
        store.record_ticket("example.com", vec![3], 2);
        assert_eq!(store.take_ticket("example.com"), Some(vec![3]));
        assert_eq!(store.take_ticket("example.com"), Some(vec![2]));
        assert!(store.take_ticket("example.com").is_none());
    }
}
