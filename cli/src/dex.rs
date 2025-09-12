use clap::Subcommand;
use hex;
use the_block::dex::{check_liquidity_rules, storage::EscrowState, DexStore};

#[derive(Subcommand)]
pub enum DexCmd {
    /// Escrow operations
    Escrow {
        #[command(subcommand)]
        action: EscrowCmd,
    },
}

#[derive(Subcommand)]
pub enum EscrowCmd {
    /// Show escrow status
    Status {
        id: u64,
        #[arg(long, default_value = "dex.bin")]
        state: String,
    },
    /// Release funds from escrow
    Release {
        id: u64,
        amount: u64,
        #[arg(long, default_value = "dex.bin")]
        state: String,
    },
}

pub fn handle(cmd: DexCmd) {
    match cmd {
        DexCmd::Escrow { action } => match action {
            EscrowCmd::Status { id, state } => {
                let store = DexStore::open(&state);
                let esc: EscrowState = store.load_escrow_state();
                if let Some(e) = esc.escrow.status(id) {
                    println!(
                        "from:{} to:{} total:{} released:{}",
                        e.from, e.to, e.total, e.released
                    );
                    for (idx, amt) in e.payments.iter().enumerate() {
                        if let Some(p) = esc.escrow.proof(id, idx) {
                            println!("payment {}: {}", idx, hex::encode(p.leaf));
                        }
                    }
                } else {
                    eprintln!("not found");
                }
            }
            EscrowCmd::Release { id, amount, state } => {
                let mut store = DexStore::open(&state);
                let mut esc: EscrowState = store.load_escrow_state();
                if let Some((_, _, _, locked_at)) = esc.locks.get(&id).cloned() {
                    if check_liquidity_rules(locked_at).is_err() {
                        eprintln!("locked");
                        return;
                    }
                }
                match esc.escrow.release(id, amount) {
                    Some(proof) => {
                        store.save_escrow_state(&esc);
                        println!("released with proof: {}", hex::encode(proof.leaf));
                    }
                    None => eprintln!("release failed"),
                }
            }
        },
    }
}
