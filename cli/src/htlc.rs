#![forbid(unsafe_code)]

use crate::parse_utils::{parse_u64_required, require_positional, take_string};
use cli_core::{
    arg::{ArgSpec, OptionSpec, PositionalSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use crypto_suite::hashing::{ripemd160, sha3::Sha3_256};
use crypto_suite::hex::{decode, encode};
use the_block::vm::contracts::htlc::{HashAlgo, Htlc};

pub enum HtlcCmd {
    /// Create an HTLC from a preimage and timeout
    Create {
        preimage: String,
        timeout: u64,
        algo: String,
    },
    /// Redeem an existing HTLC with a preimage
    Redeem {
        hash: String,
        preimage: String,
        timeout: u64,
        algo: String,
    },
}

impl HtlcCmd {
    pub fn command() -> Command {
        CommandBuilder::new(CommandId("htlc"), "htlc", "HTLC utilities")
            .subcommand(
                CommandBuilder::new(CommandId("htlc.create"), "create", "Create a new HTLC")
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "preimage",
                        "Preimage used for the HTLC",
                    )))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("timeout", "timeout", "Timeout height").default("0"),
                    ))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("algo", "algo", "Hash algorithm (sha3|ripemd)")
                            .default("sha3"),
                    ))
                    .build(),
            )
            .subcommand(
                CommandBuilder::new(CommandId("htlc.redeem"), "redeem", "Redeem an HTLC")
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "hash",
                        "Hex-encoded HTLC hash",
                    )))
                    .arg(ArgSpec::Positional(PositionalSpec::new(
                        "preimage",
                        "Preimage revealing the hash",
                    )))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("timeout", "timeout", "Timeout height").default("0"),
                    ))
                    .arg(ArgSpec::Option(
                        OptionSpec::new("algo", "algo", "Hash algorithm (sha3|ripemd)")
                            .default("sha3"),
                    ))
                    .build(),
            )
            .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'htlc'".to_string())?;

        match name {
            "create" => {
                let preimage = require_positional(sub_matches, "preimage")?;
                let timeout = parse_u64_required(take_string(sub_matches, "timeout"), "timeout")?;
                let algo = take_string(sub_matches, "algo").unwrap_or_else(|| "sha3".to_string());
                Ok(HtlcCmd::Create {
                    preimage,
                    timeout,
                    algo,
                })
            }
            "redeem" => {
                let hash = require_positional(sub_matches, "hash")?;
                let preimage = require_positional(sub_matches, "preimage")?;
                let timeout = parse_u64_required(take_string(sub_matches, "timeout"), "timeout")?;
                let algo = take_string(sub_matches, "algo").unwrap_or_else(|| "sha3".to_string());
                Ok(HtlcCmd::Redeem {
                    hash,
                    preimage,
                    timeout,
                    algo,
                })
            }
            other => Err(format!("unknown subcommand '{other}' for 'htlc'")),
        }
    }
}

pub fn handle(cmd: HtlcCmd) {
    match cmd {
        HtlcCmd::Create {
            preimage,
            timeout,
            algo,
        } => {
            let bytes = preimage.into_bytes();
            let (hash, algo) = match algo.as_str() {
                "ripemd" => match ripemd160::hash(&bytes) {
                    Ok(hash) => (hash.to_vec(), HashAlgo::Ripemd160),
                    Err(err) => {
                        eprintln!("ripemd160 hashing unavailable: {err}");
                        return;
                    }
                },
                _ => {
                    let mut h = Sha3_256::new();
                    h.update(&bytes);
                    (h.finalize().to_vec(), HashAlgo::Sha3)
                }
            };
            let htlc = Htlc::new(hash, algo, timeout);
            #[cfg(feature = "telemetry")]
            {
                the_block::telemetry::HTLC_CREATED_TOTAL.inc();
            }
            println!("{}", encode(htlc.hash));
        }
        HtlcCmd::Redeem {
            hash,
            preimage,
            timeout,
            algo,
        } => {
            let hash_bytes = decode(hash).expect("invalid hash");
            let algo = match algo.as_str() {
                "ripemd" => HashAlgo::Ripemd160,
                _ => HashAlgo::Sha3,
            };
            let mut htlc = Htlc::new(hash_bytes, algo, timeout);
            match htlc.redeem(preimage.as_bytes(), timeout.saturating_sub(1)) {
                Ok(ok) => println!("{}", ok),
                Err(err) => eprintln!("failed to redeem HTLC: {err}"),
            }
        }
    }
}
