use clap::{Parser, Subcommand};
use the_block::net::ban_store::BAN_STORE;

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

fn main() {
    let cli = Cli::parse();
    let store = BAN_STORE.lock().unwrap();
    match cli.cmd {
        Command::List => {
            for (peer, until) in store.list() {
                println!("{peer} {until}");
            }
        }
        Command::Ban { pk, secs } => {
            let arr = parse_pk(&pk);
            let until = current_ts() + secs;
            store.ban(&arr, until);
        }
        Command::Unban { pk } => {
            let arr = parse_pk(&pk);
            store.unban(&arr);
        }
    }
}

fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
