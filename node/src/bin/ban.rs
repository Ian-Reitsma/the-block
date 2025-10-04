use std::process;

use cli_core::{
    arg::{ArgSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use the_block::net::ban_store::{self, BanStoreLike};

mod cli_support;
use cli_support::{collect_args, parse_matches};

#[derive(Debug)]
struct Cli {
    cmd: Command,
}

#[derive(Debug)]
enum Command {
    List,
    Ban { pk: String, secs: u64 },
    Unban { pk: String },
}

fn parse_pk(hexstr: &str) -> [u8; 32] {
    let bytes = hex::decode(hexstr).unwrap_or_else(|e| panic!("hex pk: {e}"));
    let arr: [u8; 32] = bytes.try_into().unwrap_or_else(|_| panic!("pk length"));
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
    let command = build_command();
    let (bin, args) = collect_args("ban");
    let matches = match parse_matches(&command, &bin, args) {
        Some(matches) => matches,
        None => return,
    };

    let cli = match build_cli(matches) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };

    let store = ban_store::store().lock().unwrap_or_else(|e| e.into_inner());
    let out = run(&*store, cli.cmd);
    for (peer, until) in out {
        println!("{peer} {until}");
    }
}

fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| panic!("time error: {e}"))
        .as_secs()
}

fn build_command() -> CliCommand {
    CommandBuilder::new(CommandId("ban"), "ban", "Manage persistent peer bans")
        .subcommand(CommandBuilder::new(CommandId("ban.list"), "list", "List active bans").build())
        .subcommand(
            CommandBuilder::new(
                CommandId("ban.ban"),
                "ban",
                "Ban a peer by hex-encoded public key for N seconds",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "pk",
                "Peer public key (hex)",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "secs",
                "Ban duration in seconds",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("ban.unban"),
                "unban",
                "Remove a ban for the given hex-encoded public key",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "pk",
                "Peer public key (hex)",
            )))
            .build(),
        )
        .build()
}

fn build_cli(matches: Matches) -> Result<Cli, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    let cmd = match sub {
        "list" => Command::List,
        "ban" => {
            let pk = require_positional(sub_matches, "pk")?;
            let secs_str = require_positional(sub_matches, "secs")?;
            let secs = secs_str
                .parse::<u64>()
                .map_err(|err| format!("invalid secs value: {err}"))?;
            Command::Ban { pk, secs }
        }
        "unban" => {
            let pk = require_positional(sub_matches, "pk")?;
            Command::Unban { pk }
        }
        other => return Err(format!("unknown subcommand '{other}'")),
    };

    Ok(Cli { cmd })
}

fn require_positional(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| format!("missing argument '{name}'"))
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
            self.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(pk, until);
            self.update_metric();
        }

        fn update_metric(&self) {
            let map = self.map.lock().unwrap_or_else(|e| e.into_inner());
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
            self.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(*pk, until);
            self.update_metric();
        }

        fn unban(&self, pk: &[u8; 32]) {
            self.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(pk);
            self.update_metric();
        }

        fn list(&self) -> Vec<(String, u64)> {
            let now = current_ts();
            {
                let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
                map.retain(|_, ts| *ts > now);
            }
            self.update_metric();
            self.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
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
