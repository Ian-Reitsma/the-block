use clap::Subcommand;
use the_block::dex::DexStore;
use dex::escrow::Escrow;

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
                let mut store = DexStore::open(&state);
                let esc: Escrow = store.load_escrow();
                if let Some(e) = esc.status(id) {
                    println!(
                        "from:{} to:{} total:{} released:{}",
                        e.from, e.to, e.total, e.released
                    );
                } else {
                    eprintln!("not found");
                }
            }
            EscrowCmd::Release { id, amount, state } => {
                let mut store = DexStore::open(&state);
                let mut esc: Escrow = store.load_escrow();
                match esc.release(id, amount) {
                    Some(proof) => {
                        store.save_escrow(&esc);
                        println!("released with proof: {}", hex::encode(proof.leaf));
                    }
                    None => eprintln!("release failed"),
                }
            }
        },
    }
}
