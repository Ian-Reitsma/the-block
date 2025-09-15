use clap::Subcommand;
use hex;
use the_block::dex::{check_liquidity_rules, storage::EscrowState, DexStore};
use the_block::dex::amm::Pool;

#[derive(Subcommand)]
pub enum DexCmd {
    /// Escrow operations
    Escrow {
        #[command(subcommand)]
        action: EscrowCmd,
    },
    /// Liquidity pool operations
    Liquidity {
        #[command(subcommand)]
        action: LiquidityCmd,
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

#[derive(Subcommand)]
pub enum LiquidityCmd {
    /// Add liquidity to a pool
    Add {
        pool: String,
        ct: u64,
        it: u64,
        #[arg(long, default_value = "dex.bin")]
        state: String,
    },
    /// Remove liquidity from a pool
    Remove {
        pool: String,
        shares: u64,
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
        DexCmd::Liquidity { action } => match action {
            LiquidityCmd::Add { pool, ct, it, state } => {
                let mut store = DexStore::open(&state);
                let mut p: Pool = store.load_pool(&pool);
                let minted = p.add_liquidity(ct as u128, it as u128);
                store.save_pool(&pool, &p);
                println!("minted shares: {}", minted);
            }
            LiquidityCmd::Remove { pool, shares, state } => {
                let mut store = DexStore::open(&state);
                let mut p: Pool = store.load_pool(&pool);
                let (ct, it) = p.remove_liquidity(shares as u128);
                store.save_pool(&pool, &p);
                println!("withdrawn ct:{} it:{}", ct, it);
            }
        },
    }
}
