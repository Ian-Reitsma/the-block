use clap::{Parser, Subcommand};
use the_block::net::ban_store::{self, BanStoreLike};

#[derive(Parser)]
#[command(author, version, about = "Manage persistent peer bans")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List active bans
    List,
    /// Ban a peer by hex-encoded public key for N seconds
    Ban { pk: String, secs: u64 },
    /// Remove a ban for the given hex-encoded public key
    Unban { pk: String },
}

fn parse_pk(hexstr: &str) -> [u8; 32] {
    let bytes = hex::decode(hexstr).expect("hex pk");
    let arr: [u8; 32] = bytes.try_into().expect("pk length");
    arr
}

fn run<S: BanStoreLike>(store: &S, cmd: Command) -> Vec<(String, u64)> {
    match cmd {
        Command::List => store.list(),
        Command::Ban { pk, secs } => {
            let arr = parse_pk(&pk);
            let until = current_ts() + secs;
            store.ban(&arr, until);
            Vec::new()
        }
        Command::Unban { pk } => {
            let arr = parse_pk(&pk);
            store.unban(&arr);
            Vec::new()
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let store = ban_store::store().lock().unwrap();
    let out = run(&*store, cli.cmd);
    for (peer, until) in out {
        println!("{peer} {until}");
    }
}

fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use serial_test::serial;
    use the_block::telemetry::{BANNED_PEERS_TOTAL, BANNED_PEER_EXPIRATION};

    #[derive(Default)]
    struct MockStore {
        map: Mutex<HashMap<[u8; 32], u64>>,
    }

    impl MockStore {
        fn insert_raw(&self, pk: [u8; 32], until: u64) {
            self.map.lock().unwrap().insert(pk, until);
            self.update_metric();
        }

        fn update_metric(&self) {
            let map = self.map.lock().unwrap();
            BANNED_PEERS_TOTAL.set(map.len() as i64);
            BANNED_PEER_EXPIRATION.reset();
            for (k, v) in map.iter() {
                BANNED_PEER_EXPIRATION
                    .with_label_values(&[&hex::encode(k)])
                    .set(*v as i64);
            }
        }
    }

    impl BanStoreLike for MockStore {
        fn ban(&self, pk: &[u8; 32], until: u64) {
            self.map.lock().unwrap().insert(*pk, until);
            self.update_metric();
        }

        fn unban(&self, pk: &[u8; 32]) {
            self.map.lock().unwrap().remove(pk);
            self.update_metric();
        }

        fn list(&self) -> Vec<(String, u64)> {
            let now = current_ts();
            {
                let mut map = self.map.lock().unwrap();
                map.retain(|_, ts| *ts > now);
            }
            self.update_metric();
            self.map
                .lock()
                .unwrap()
                .iter()
                .map(|(k, v)| (hex::encode(k), *v))
                .collect()
        }
    }

    fn reset_metrics() {
        BANNED_PEERS_TOTAL.set(0);
        BANNED_PEER_EXPIRATION.reset();
    }

    #[test]
    #[serial]
    fn ban_and_unban_update_metrics() {
        reset_metrics();
        let store = MockStore::default();
        let pk = hex::encode([1u8; 32]);
        run(
            &store,
            Command::Ban {
                pk: pk.clone(),
                secs: 60,
            },
        );
        store.list();
        assert_eq!(BANNED_PEERS_TOTAL.get(), 1);
        run(&store, Command::Unban { pk });
        store.list();
        assert_eq!(BANNED_PEERS_TOTAL.get(), 0);
    }

    #[test]
    #[serial]
    fn list_purges_expired_bans() {
        reset_metrics();
        let store = MockStore::default();
        let pk = [2u8; 32];
        store.insert_raw(pk, current_ts() - 1);
        let out = run(&store, Command::List);
        assert!(out.is_empty());
        assert_eq!(BANNED_PEERS_TOTAL.get(), 0);
    }
}
