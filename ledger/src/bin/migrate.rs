use clap::Parser;
use ledger::utxo_account::migrate_accounts;
use serde_json;
use std::{collections::HashMap, fs};

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    input: String,
    #[arg(short, long)]
    output: String,
}

fn main() {
    let args = Args::parse();
    let data = fs::read_to_string(&args.input).expect("read input");
    let balances: HashMap<String, u64> = serde_json::from_str(&data).expect("parse input");
    let utxo = migrate_accounts(&balances);
    let out = serde_json::to_string_pretty(&utxo).expect("serialize");
    fs::write(&args.output, out).expect("write output");
}
