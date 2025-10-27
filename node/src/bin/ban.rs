use std::process;

use cli_core::{
    arg::{ArgSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};
use the_block::net::ban_store::{self, BanStoreError, BanStoreLike};

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
    let bytes = crypto_suite::hex::decode(hexstr).unwrap_or_else(|e| panic!("hex pk: {e}"));
    let arr: [u8; 32] = bytes.try_into().unwrap_or_else(|_| panic!("pk length"));
    arr
}

fn run<S: BanStoreLike>(store: &S, cmd: Command) -> Result<Vec<(String, u64)>, BanStoreError> {
    match cmd {
        Command::List => store.list(),
        Command::Ban { pk, secs } => {
            let arr = parse_pk(&pk);
            let until = current_ts() + secs;
            store.ban(&arr, until)?;
            Ok(Vec::new())
        }
        Command::Unban { pk } => {
            let arr = parse_pk(&pk);
            store.unban(&arr)?;
            Ok(Vec::new())
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
    let out = match run(&*store, cli.cmd) {
        Ok(out) => out,
        Err(err) => {
            eprintln!("ban store error: {err}");
            process::exit(1);
        }
    };
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
    use std::io::{self, Write};
    use std::sync::Mutex;

    use cli_core::{
        help::HelpGenerator,
        parse::{ParseError, Parser},
    };
    use the_block::telemetry::{self, BANNED_PEERS_TOTAL, BANNED_PEER_EXPIRATION};

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
                    .ensure_handle_for_label_values(&[&crypto_suite::hex::encode(k)])
                    .expect(telemetry::LABEL_REGISTRATION_ERR)
                    .set(*v as i64);
            }
        }
    }

    impl BanStoreLike for MockStore {
        fn ban(&self, pk: &[u8; 32], until: u64) -> Result<(), BanStoreError> {
            self.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(*pk, until);
            self.update_metric();
            Ok(())
        }

        fn unban(&self, pk: &[u8; 32]) -> Result<(), BanStoreError> {
            self.map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(pk);
            self.update_metric();
            Ok(())
        }

        fn list(&self) -> Result<Vec<(String, u64)>, BanStoreError> {
            let now = current_ts();
            {
                let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
                map.retain(|_, ts| *ts > now);
            }
            self.update_metric();
            let list = self
                .map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .map(|(k, v)| (crypto_suite::hex::encode(k), *v))
                .collect();
            Ok(list)
        }
    }

    fn reset_metrics() {
        BANNED_PEERS_TOTAL.set(0);
        BANNED_PEER_EXPIRATION.reset();
    }

    fn run_cli_for_test<S: BanStoreLike>(store: &S, args: &[&str]) -> (i32, String, String) {
        let command = build_command();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        if args.is_empty() {
            let generator = HelpGenerator::new(&command);
            writeln!(&mut stdout, "{}", generator.render()).unwrap();
            writeln!(
                &mut stdout,
                "\nRun 'ban <subcommand> --help' for details on a command."
            )
            .unwrap();
            return (
                0,
                String::from_utf8(stdout).unwrap(),
                String::from_utf8(stderr).unwrap(),
            );
        }

        let argv: Vec<String> = args.iter().map(|value| value.to_string()).collect();
        let parser = Parser::new(&command);
        let matches = match parser.parse(&argv) {
            Ok(matches) => matches,
            Err(ParseError::HelpRequested(path)) => {
                let segments: Vec<&str> = path.split_whitespace().collect();
                let mut current = &command;
                for segment in segments.iter().skip(1) {
                    if let Some(next) = current.subcommands.iter().find(|cmd| cmd.name == *segment)
                    {
                        current = next;
                    } else {
                        current = &command;
                        break;
                    }
                }
                let generator = HelpGenerator::new(current);
                writeln!(&mut stdout, "{}", generator.render()).unwrap();
                return (
                    0,
                    String::from_utf8(stdout).unwrap(),
                    String::from_utf8(stderr).unwrap(),
                );
            }
            Err(err) => {
                writeln!(&mut stderr, "{err}").unwrap();
                return (
                    2,
                    String::from_utf8(stdout).unwrap(),
                    String::from_utf8(stderr).unwrap(),
                );
            }
        };

        let cli = match build_cli(matches) {
            Ok(cli) => cli,
            Err(err) => {
                writeln!(&mut stderr, "{err}").unwrap();
                return (
                    2,
                    String::from_utf8(stdout).unwrap(),
                    String::from_utf8(stderr).unwrap(),
                );
            }
        };

        match run(store, cli.cmd) {
            Ok(entries) => {
                for (peer, until) in entries {
                    writeln!(&mut stdout, "{peer} {until}").unwrap();
                }
                (
                    0,
                    String::from_utf8(stdout).unwrap(),
                    String::from_utf8(stderr).unwrap(),
                )
            }
            Err(err) => {
                writeln!(&mut stderr, "ban store error: {err}").unwrap();
                (
                    1,
                    String::from_utf8(stdout).unwrap(),
                    String::from_utf8(stderr).unwrap(),
                )
            }
        }
    }

    #[derive(Clone, Copy)]
    enum FailingOperation {
        Ban,
        Unban,
        List,
    }

    struct FailingStore {
        failure: FailingOperation,
    }

    impl FailingStore {
        fn new(failure: FailingOperation) -> Self {
            Self { failure }
        }

        fn storage_error(context: &'static str) -> BanStoreError {
            BanStoreError::Storage(io::Error::new(io::ErrorKind::Other, context))
        }
    }

    impl BanStoreLike for FailingStore {
        fn ban(&self, _pk: &[u8; 32], _until: u64) -> Result<(), BanStoreError> {
            match self.failure {
                FailingOperation::Ban => Err(Self::storage_error("ban failure")),
                _ => Ok(()),
            }
        }

        fn unban(&self, _pk: &[u8; 32]) -> Result<(), BanStoreError> {
            match self.failure {
                FailingOperation::Unban => Err(Self::storage_error("unban failure")),
                _ => Ok(()),
            }
        }

        fn list(&self) -> Result<Vec<(String, u64)>, BanStoreError> {
            match self.failure {
                FailingOperation::List => Err(Self::storage_error("list failure")),
                _ => Ok(Vec::new()),
            }
        }
    }

    #[testkit::tb_serial]
    fn ban_and_unban_update_metrics() {
        reset_metrics();
        let store = MockStore::default();
        let pk = crypto_suite::hex::encode([1u8; 32]);
        run(
            &store,
            Command::Ban {
                pk: pk.clone(),
                secs: 60,
            },
        )
        .expect("ban");
        store.list().expect("list");
        assert_eq!(BANNED_PEERS_TOTAL.value(), 1);
        run(&store, Command::Unban { pk }).expect("unban");
        store.list().expect("list");
        assert_eq!(BANNED_PEERS_TOTAL.value(), 0);
    }

    #[testkit::tb_serial]
    fn list_purges_expired_bans() {
        reset_metrics();
        let store = MockStore::default();
        let pk = [2u8; 32];
        store.insert_raw(pk, current_ts() - 1);
        let out = run(&store, Command::List).expect("list");
        assert!(out.is_empty());
        assert_eq!(BANNED_PEERS_TOTAL.value(), 0);
    }

    #[testkit::tb_serial]
    fn cli_ban_succeeds_and_updates_store() {
        reset_metrics();
        let store = MockStore::default();
        let pk = crypto_suite::hex::encode([7u8; 32]);
        let (code, stdout, stderr) = run_cli_for_test(&store, &["ban", &pk, "30"]);
        assert_eq!(code, 0);
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        let entries = store.list().expect("list after ban");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, pk);
    }

    #[testkit::tb_serial]
    fn cli_unban_succeeds_and_clears_store() {
        reset_metrics();
        let store = MockStore::default();
        let pk = [8u8; 32];
        store.insert_raw(pk, current_ts() + 30);
        let hex_pk = crypto_suite::hex::encode(pk);
        let (code, stdout, stderr) = run_cli_for_test(&store, &["unban", &hex_pk]);
        assert_eq!(code, 0);
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        let entries = store.list().expect("list after unban");
        assert!(entries.is_empty());
    }

    #[testkit::tb_serial]
    fn cli_list_outputs_active_bans() {
        reset_metrics();
        let store = MockStore::default();
        let pk = [9u8; 32];
        let until = current_ts() + 120;
        store.insert_raw(pk, until);
        let hex_pk = crypto_suite::hex::encode(pk);
        let (code, stdout, stderr) = run_cli_for_test(&store, &["list"]);
        assert_eq!(code, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains(&hex_pk));
        assert!(stdout.contains(&until.to_string()));
    }

    #[testkit::tb_serial]
    fn ban_error_surfaces_without_touching_metrics() {
        reset_metrics();
        let store = FailingStore::new(FailingOperation::Ban);
        let err = run(
            &store,
            Command::Ban {
                pk: crypto_suite::hex::encode([3u8; 32]),
                secs: 5,
            },
        )
        .expect_err("ban should bubble storage error");
        assert!(
            matches!(err, BanStoreError::Storage(_)),
            "expected storage error"
        );
        assert_eq!(BANNED_PEERS_TOTAL.value(), 0);
    }

    #[testkit::tb_serial]
    fn unban_error_surfaces_without_touching_metrics() {
        reset_metrics();
        let store = FailingStore::new(FailingOperation::Unban);
        let err = run(
            &store,
            Command::Unban {
                pk: crypto_suite::hex::encode([4u8; 32]),
            },
        )
        .expect_err("unban should bubble storage error");
        assert!(
            matches!(err, BanStoreError::Storage(_)),
            "expected storage error"
        );
        assert_eq!(BANNED_PEERS_TOTAL.value(), 0);
    }

    #[testkit::tb_serial]
    fn list_error_propagates() {
        reset_metrics();
        let store = FailingStore::new(FailingOperation::List);
        let err = run(&store, Command::List).expect_err("list should bubble storage error");
        assert!(
            matches!(err, BanStoreError::Storage(_)),
            "expected storage error"
        );
        assert_eq!(BANNED_PEERS_TOTAL.value(), 0);
    }

    #[testkit::tb_serial]
    fn cli_reports_ban_failure_to_stderr() {
        reset_metrics();
        let store = FailingStore::new(FailingOperation::Ban);
        let pk = crypto_suite::hex::encode([5u8; 32]);
        let (code, stdout, stderr) = run_cli_for_test(&store, &["ban", &pk, "5"]);
        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        assert!(
            stderr.contains("ban store error: ban store storage error: ban failure"),
            "stderr captured: {stderr}"
        );
    }

    #[testkit::tb_serial]
    fn cli_reports_unban_failure_to_stderr() {
        reset_metrics();
        let store = FailingStore::new(FailingOperation::Unban);
        let pk = crypto_suite::hex::encode([6u8; 32]);
        let (code, stdout, stderr) = run_cli_for_test(&store, &["unban", &pk]);
        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        assert!(
            stderr.contains("ban store error: ban store storage error: unban failure"),
            "stderr captured: {stderr}"
        );
    }

    #[testkit::tb_serial]
    fn cli_reports_list_failure_to_stderr() {
        reset_metrics();
        let store = FailingStore::new(FailingOperation::List);
        let (code, stdout, stderr) = run_cli_for_test(&store, &["list"]);
        assert_eq!(code, 1);
        assert!(stdout.is_empty());
        assert!(
            stderr.contains("ban store error: ban store storage error: list failure"),
            "stderr captured: {stderr}"
        );
    }
}
