use crate::parse_utils::{parse_positional_u64, require_positional, take_string};
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use dex::amm::Pool;
use the_block::dex::{check_liquidity_rules, storage::EscrowState, DexStore};

pub enum DexCmd {
    /// Escrow operations
    Escrow { action: EscrowCmd },
    /// Liquidity pool operations
    Liquidity { action: LiquidityCmd },
}

pub enum EscrowCmd {
    /// Show escrow status
    Status { id: u64, state: String },
    /// Release funds from escrow
    Release { id: u64, amount: u64, state: String },
}

pub enum LiquidityCmd {
    /// Add liquidity to a pool
    Add {
        pool: String,
        ct: u64,
        it: u64,
        state: String,
    },
    /// Remove liquidity from a pool
    Remove {
        pool: String,
        shares: u64,
        state: String,
    },
}

impl DexCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("dex"), "dex", "DEX escrow utilities")
            .subcommand(EscrowCmd::command())
            .subcommand(LiquidityCmd::command())
            .build()
    }

    pub fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'dex'".to_string())?;

        match name {
            "escrow" => Ok(DexCmd::Escrow {
                action: EscrowCmd::from_matches(sub_matches)?,
            }),
            "liquidity" => Ok(DexCmd::Liquidity {
                action: LiquidityCmd::from_matches(sub_matches)?,
            }),
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

impl EscrowCmd {
    fn command() -> Command {
        CommandBuilder::new(CommandId("dex.escrow"), "escrow", "Escrow operations")
            .subcommand(
                CommandBuilder::new(
                    CommandId("dex.escrow.status"),
                    "status",
                    "Show escrow status",
                )
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "id",
                    "Escrow identifier",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("state", "state", "Dex state file").default("dex.bin"),
                ))
                .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("dex.escrow.release"), "release", "Release funds")
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "id",
                        "Escrow identifier",
                    )))
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "amount",
                        "Amount to release",
                    )))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("state", "state", "Dex state file").default("dex.bin"),
                    ))
                    .build(),
            )
            .build()
    }

    fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'dex escrow'".to_string())?;

        match name {
            "status" => {
                let id = parse_positional_u64(sub_matches, "id")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "dex.bin".to_string());
                Ok(EscrowCmd::Status { id, state })
            }
            "release" => {
                let id = parse_positional_u64(sub_matches, "id")?;
                let amount = parse_positional_u64(sub_matches, "amount")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "dex.bin".to_string());
                Ok(EscrowCmd::Release { id, amount, state })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
}

impl LiquidityCmd {
    fn command() -> Command {
        CommandBuilder::new(
            CommandId("dex.liquidity"),
            "liquidity",
            "Liquidity pool operations",
        )
        .subcommand(
            CommandBuilder::new(CommandId("dex.liquidity.add"), "add", "Add liquidity")
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "pool",
                    "Pool identifier",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "ct",
                    "CT contribution",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "it",
                    "Industrial contribution",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("state", "state", "Dex state file").default("dex.bin"),
                ))
                .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("dex.liquidity.remove"),
                "remove",
                "Remove liquidity",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "pool",
                "Pool identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "shares",
                "Shares to burn",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("state", "state", "Dex state file").default("dex.bin"),
            ))
            .build(),
        )
        .build()
    }

    fn from_matches(matches: &Matches) -> std::result::Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'dex liquidity'".to_string())?;

        match name {
            "add" => {
                let pool = require_positional(sub_matches, "pool")?;
                let ct = parse_positional_u64(sub_matches, "ct")?;
                let it = parse_positional_u64(sub_matches, "it")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "dex.bin".to_string());
                Ok(LiquidityCmd::Add {
                    pool,
                    ct,
                    it,
                    state,
                })
            }
            "remove" => {
                let pool = require_positional(sub_matches, "pool")?;
                let shares = parse_positional_u64(sub_matches, "shares")?;
                let state =
                    take_string(sub_matches, "state").unwrap_or_else(|| "dex.bin".to_string());
                Ok(LiquidityCmd::Remove {
                    pool,
                    shares,
                    state,
                })
            }
            other => Err(format!("unknown subcommand '{other}'")),
        }
    }
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
                    for (idx, amount) in e.payments.iter().enumerate() {
                        if let Some(p) = esc.escrow.proof(id, idx) {
                            println!(
                                "payment {}: {} amount:{}",
                                idx,
                                crypto_suite::hex::encode(p.leaf),
                                amount
                            );
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
                        println!(
                            "released with proof: {}",
                            crypto_suite::hex::encode(proof.leaf)
                        );
                    }
                    None => eprintln!("release failed"),
                }
            }
        },
        DexCmd::Liquidity { action } => match action {
            LiquidityCmd::Add {
                pool,
                ct,
                it,
                state,
            } => {
                let mut store = DexStore::open(&state);
                let mut p: Pool = store.load_pool(&pool);
                let minted = p.add_liquidity(ct as u128, it as u128);
                store.save_pool(&pool, &p);
                println!("minted shares: {}", minted);
            }
            LiquidityCmd::Remove {
                pool,
                shares,
                state,
            } => {
                let mut store = DexStore::open(&state);
                let mut p: Pool = store.load_pool(&pool);
                let (ct, it) = p.remove_liquidity(shares as u128);
                store.save_pool(&pool, &p);
                println!("withdrawn ct:{} it:{}", ct, it);
            }
        },
    }
}
