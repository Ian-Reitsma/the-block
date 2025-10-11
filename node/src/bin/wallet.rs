#![deny(warnings)]

use crypto_suite::hex::{decode, encode};
use crypto_suite::signatures::ed25519::Signature;
use dex::escrow::{verify_proof, PaymentProof};
use foundation_serialization::json::{self, Value};
use httpd::{BlockingClient, Method};
use node::http_client;
use wallet::{hardware::MockHardwareWallet, remote_signer::RemoteSigner, Wallet, WalletSigner};

use the_block::storage::pipeline::{Provider, StoragePipeline};
use the_block::storage::placement::NodeCatalog;

use cli_core::{
    arg::{ArgSpec, FlagSpec, OptionSpec, PositionalSpec},
    command::{Command as CliCommand, CommandBuilder, CommandId},
    parse::Matches,
};

mod cli_support;
use cli_support::{collect_args, parse_matches, print_root_help};

const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8545";
const DEFAULT_ESCROW_RELEASE_URL: &str = "http://127.0.0.1:26658";

/// Simple CLI for wallet operations.
#[derive(Debug)]
struct Cli {
    command: Commands,
}

#[derive(Debug)]
enum Commands {
    /// Generate a new wallet and print the public key as hex.
    Generate,
    /// Sign a message given a hex-encoded seed and print the signature as hex.
    Sign {
        seed: Option<String>,
        message: String,
        remote_signer: Vec<String>,
        signer_cert: Option<String>,
        signer_key: Option<String>,
        signer_ca: Option<String>,
        threshold: usize,
    },
    /// Sign a message using a mock hardware wallet.
    SignHw { message: String },
    /// Stake CT for a service role. Remote signers submit `signers[]` plus a
    /// `threshold` field so the staking RPC can validate multi-party
    /// approvals.
    StakeRole {
        role: Role,
        amount: u64,
        seed: Option<String>,
        remote_signer: Vec<String>,
        signer_cert: Option<String>,
        signer_key: Option<String>,
        signer_ca: Option<String>,
        withdraw: bool,
        url: String,
        threshold: usize,
    },
    /// Query rent-escrow balance for an account
    EscrowBalance { account: String, url: String },
    /// Release funds from a DEX escrow, verifying the provided proof
    EscrowRelease { id: u64, amount: u64, url: String },
    /// Chunk a file and build a BlobTx, printing the blob root
    BlobPut { file: String, owner: String },
    /// Retrieve a blob by its manifest hash and write to an output file
    BlobGet { blob_id: String, out: String },
    /// Show help information
    Help,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Role {
    Gateway,
    Storage,
    Exec,
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "gateway" => Ok(Role::Gateway),
            "storage" => Ok(Role::Storage),
            "exec" => Ok(Role::Exec),
            other => Err(format!("invalid role '{other}'")),
        }
    }
}

fn build_command() -> CliCommand {
    CommandBuilder::new(CommandId("wallet"), "wallet", "Wallet operations")
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.generate"),
                "generate",
                "Generate a new wallet",
            )
            .build(),
        )
        .subcommand(
            CommandBuilder::new(CommandId("wallet.sign"), "sign", "Sign a message")
                .arg(ArgSpec::Option(OptionSpec::new(
                    "seed",
                    "seed",
                    "32-byte seed in hex",
                )))
                .arg(ArgSpec::Positional(PositionalSpec::new(
                    "message",
                    "Message to sign",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new(
                        "remote_signer",
                        "remote-signer",
                        "Remote signer endpoint (repeatable)",
                    )
                    .multiple(true),
                ))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "signer_cert",
                    "signer-cert",
                    "Client TLS certificate (PEM)",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "signer_key",
                    "signer-key",
                    "Client TLS private key (PEM)",
                )))
                .arg(ArgSpec::Option(OptionSpec::new(
                    "signer_ca",
                    "signer-ca",
                    "CA certificate for remote signer",
                )))
                .arg(ArgSpec::Option(
                    OptionSpec::new("threshold", "threshold", "Remote signer threshold")
                        .default("1"),
                ))
                .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.sign_hw"),
                "sign-hw",
                "Sign a message using a mock hardware wallet",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "message",
                "Message to sign",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.stake_role"),
                "stake-role",
                "Stake CT for a service role",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "role",
                "Service role (gateway|storage|exec)",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Amount to stake",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "seed",
                "seed",
                "32-byte seed in hex",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "remote_signer",
                    "remote-signer",
                    "Remote signer endpoint (repeatable)",
                )
                .multiple(true),
            ))
            .arg(ArgSpec::Option(OptionSpec::new(
                "signer_cert",
                "signer-cert",
                "Client TLS certificate (PEM)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "signer_key",
                "signer-key",
                "Client TLS private key (PEM)",
            )))
            .arg(ArgSpec::Option(OptionSpec::new(
                "signer_ca",
                "signer-ca",
                "CA certificate for remote signer",
            )))
            .arg(ArgSpec::Flag(FlagSpec::new(
                "withdraw",
                "withdraw",
                "Withdraw instead of bond",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "Wallet RPC endpoint").default(DEFAULT_RPC_URL),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("threshold", "threshold", "Remote signer threshold").default("1"),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.escrow_balance"),
                "escrow-balance",
                "Query rent-escrow balance for an account",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "account",
                "Account identifier",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "Wallet RPC endpoint").default(DEFAULT_RPC_URL),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.escrow_release"),
                "escrow-release",
                "Release funds from a DEX escrow",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "id",
                "Escrow identifier",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "amount",
                "Amount to release",
            )))
            .arg(ArgSpec::Option(
                OptionSpec::new("url", "url", "DEX escrow endpoint")
                    .default(DEFAULT_ESCROW_RELEASE_URL),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.blob_put"),
                "blob-put",
                "Chunk a file and build a BlobTx",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "file",
                "File to upload",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "owner",
                "Owner account",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("wallet.blob_get"),
                "blob-get",
                "Retrieve a blob by its manifest hash",
            )
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "blob_id",
                "Blob manifest hash",
            )))
            .arg(ArgSpec::Positional(PositionalSpec::new(
                "out",
                "Output file",
            )))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(CommandId("wallet.help"), "help", "Show help information").build(),
        )
        .build()
}

struct DummyProvider {
    id: String,
}

impl Provider for DummyProvider {
    fn id(&self) -> &str {
        &self.id
    }
}

fn build_cli(matches: Matches) -> Result<Cli, String> {
    let (sub, sub_matches) = matches
        .subcommand()
        .ok_or_else(|| "missing subcommand".to_string())?;

    let command = match sub {
        "generate" => Commands::Generate,
        "sign" => parse_sign(sub_matches)?,
        "sign-hw" => Commands::SignHw {
            message: require_positional(sub_matches, "message")?,
        },
        "stake-role" => parse_stake_role(sub_matches)?,
        "escrow-balance" => Commands::EscrowBalance {
            account: require_positional(sub_matches, "account")?,
            url: sub_matches
                .get_string("url")
                .unwrap_or_else(|| DEFAULT_RPC_URL.to_string()),
        },
        "escrow-release" => Commands::EscrowRelease {
            id: parse_u64(&require_positional(sub_matches, "id")?, "id")?,
            amount: parse_u64(&require_positional(sub_matches, "amount")?, "amount")?,
            url: sub_matches
                .get_string("url")
                .unwrap_or_else(|| DEFAULT_ESCROW_RELEASE_URL.to_string()),
        },
        "blob-put" => Commands::BlobPut {
            file: require_positional(sub_matches, "file")?,
            owner: require_positional(sub_matches, "owner")?,
        },
        "blob-get" => Commands::BlobGet {
            blob_id: require_positional(sub_matches, "blob_id")?,
            out: require_positional(sub_matches, "out")?,
        },
        "help" => Commands::Help,
        other => return Err(format!("unknown subcommand '{other}'")),
    };

    Ok(Cli { command })
}

fn parse_sign(matches: &Matches) -> Result<Commands, String> {
    let seed = matches.get_string("seed");
    let remote_signer = matches.get_strings("remote_signer");
    if seed.is_some() && !remote_signer.is_empty() {
        return Err("--seed cannot be used with --remote-signer".to_string());
    }

    if remote_signer.is_empty() && matches.get_string("signer_cert").is_some() {
        return Err("--signer-cert requires --remote-signer".to_string());
    }
    if remote_signer.is_empty() && matches.get_string("signer_key").is_some() {
        return Err("--signer-key requires --remote-signer".to_string());
    }

    let signer_cert = matches.get_string("signer_cert");
    let signer_key = matches.get_string("signer_key");
    if signer_cert.is_some() ^ signer_key.is_some() {
        return Err("--signer-cert and --signer-key must be provided together".to_string());
    }

    let threshold = matches
        .get_string("threshold")
        .unwrap_or_else(|| "1".to_string())
        .parse::<usize>()
        .map_err(|err| format!("invalid threshold: {err}"))?;

    Ok(Commands::Sign {
        seed,
        message: require_positional(matches, "message")?,
        remote_signer,
        signer_cert,
        signer_key,
        signer_ca: matches.get_string("signer_ca"),
        threshold,
    })
}

fn parse_stake_role(matches: &Matches) -> Result<Commands, String> {
    let seed = matches.get_string("seed");
    let remote_signer = matches.get_strings("remote_signer");

    if seed.is_none() && remote_signer.is_empty() {
        return Err("either --seed or --remote-signer must be provided".to_string());
    }
    if seed.is_some() && !remote_signer.is_empty() {
        return Err("--seed cannot be combined with --remote-signer".to_string());
    }

    let signer_cert = matches.get_string("signer_cert");
    let signer_key = matches.get_string("signer_key");
    if signer_cert.is_some() ^ signer_key.is_some() {
        return Err("--signer-cert and --signer-key must be provided together".to_string());
    }

    let role = require_positional(matches, "role")?.parse::<Role>()?;
    let amount = parse_u64(&require_positional(matches, "amount")?, "amount")?;
    let threshold = matches
        .get_string("threshold")
        .unwrap_or_else(|| "1".to_string())
        .parse::<usize>()
        .map_err(|err| format!("invalid threshold: {err}"))?;

    Ok(Commands::StakeRole {
        role,
        amount,
        seed,
        remote_signer,
        signer_cert,
        signer_key,
        signer_ca: matches.get_string("signer_ca"),
        withdraw: matches.get_flag("withdraw"),
        url: matches
            .get_string("url")
            .unwrap_or_else(|| DEFAULT_RPC_URL.to_string()),
        threshold,
    })
}

fn require_positional(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_positional(name)
        .and_then(|values| values.first().cloned())
        .ok_or_else(|| format!("missing argument '{name}'"))
}

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|err| format!("invalid {name}: {err}"))
}

fn main() {
    let command = build_command();
    let (bin, args) = collect_args("wallet");
    let matches = match parse_matches(&command, &bin, args) {
        Some(matches) => matches,
        None => return,
    };

    let cli = match build_cli(matches) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    match cli.command {
        Commands::Generate => {
            let wallet = Wallet::generate();
            println!("{}", encode(wallet.public_key()));
        }
        Commands::Sign {
            seed,
            message,
            remote_signer,
            signer_cert,
            signer_key,
            signer_ca,
            threshold,
        } => {
            if !remote_signer.is_empty() {
                if let Some(cert) = signer_cert {
                    std::env::set_var("REMOTE_SIGNER_TLS_CERT", cert);
                }
                if let Some(key) = signer_key {
                    std::env::set_var("REMOTE_SIGNER_TLS_KEY", key);
                }
                if let Some(ca) = signer_ca {
                    std::env::set_var("REMOTE_SIGNER_TLS_CA", ca);
                }
                let signer =
                    RemoteSigner::connect_multi(&remote_signer, threshold).expect("connect signer");
                let sigs = signer.sign_multisig(message.as_bytes()).expect("sign");
                let agg: Vec<u8> = sigs
                    .into_iter()
                    .flat_map(|(_, sig)| sig.to_bytes())
                    .collect();
                println!("{}", encode(agg));
            } else {
                let seed = seed.expect("seed required");
                let seed_bytes = decode(&seed).expect("hex seed");
                assert_eq!(seed_bytes.len(), 32, "seed must be 32 bytes");
                let mut seed_arr = [0u8; 32];
                seed_arr.copy_from_slice(&seed_bytes);
                let wallet = Wallet::from_seed(&seed_arr);
                let sig = wallet.sign(message.as_bytes()).expect("sign");
                println!("{}", encode(sig.to_bytes()));
            }
        }
        Commands::SignHw { message } => {
            let mut hw = MockHardwareWallet::new();
            hw.connect();
            let sig = hw.sign(message.as_bytes()).expect("sign");
            println!("{}", encode(sig.to_bytes()));
        }
        Commands::StakeRole {
            role,
            amount,
            seed,
            remote_signer,
            signer_cert,
            signer_key,
            signer_ca,
            withdraw,
            url,
            threshold,
        } => {
            let role_str = format!("{:?}", role).to_lowercase();
            let sig;
            let id;
            let signers_payload: Vec<Value>;
            let threshold_value: usize;
            if !remote_signer.is_empty() {
                if let Some(cert) = signer_cert {
                    std::env::set_var("REMOTE_SIGNER_TLS_CERT", cert);
                }
                if let Some(key) = signer_key {
                    std::env::set_var("REMOTE_SIGNER_TLS_KEY", key);
                }
                if let Some(ca) = signer_ca {
                    std::env::set_var("REMOTE_SIGNER_TLS_CA", ca);
                }
                let signer =
                    RemoteSigner::connect_multi(&remote_signer, threshold).expect("connect signer");
                let action = if withdraw { "unbond" } else { "bond" };
                let msg = format!("{action}:{role_str}:{amount}");
                let approvals = signer.sign_multisig(msg.as_bytes()).expect("sign");
                let primary = approvals
                    .first()
                    .expect("remote signer returned no approvals");
                id = encode(primary.0.to_bytes());
                sig = primary.1.clone();
                signers_payload = approvals
                    .iter()
                    .map(|(pk, sig)| {
                        foundation_serialization::json!({
                            "pk": encode(pk.to_bytes()),
                            "sig": encode(sig.to_bytes()),
                        })
                    })
                    .collect();
                threshold_value = signer.threshold();
            } else {
                let seed = seed.expect("seed required");
                let bytes = decode(&seed).expect("seed hex");
                assert!(bytes.len() >= 32, "seed too short");
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes[..32]);
                let wallet = Wallet::from_seed(&arr);
                sig = wallet
                    .sign_stake(&role_str, amount, withdraw)
                    .expect("sign");
                id = wallet.public_key_hex();
                signers_payload = vec![foundation_serialization::json!({
                    "pk": id.clone(),
                    "sig": encode(sig.to_bytes()),
                })];
                threshold_value = 1;
            }
            let sig_hex = encode(sig.to_bytes());
            let body = foundation_serialization::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": if withdraw { "consensus.pos.unbond" } else { "consensus.pos.bond" },
                "params": {
                    "id": id,
                    "role": role_str,
                    "amount": amount,
                    "sig": sig_hex,
                    "signers": signers_payload,
                    "threshold": threshold_value,
                }
            });
            let client = http_client::blocking_client();
            match client
                .request(Method::Post, &url)
                .and_then(|builder| builder.json(&body))
                .and_then(|builder| builder.send())
            {
                Ok(resp) => match resp.json::<Value>() {
                    Ok(v) => println!("{}", v["result"].to_string()),
                    Err(e) => eprintln!("parse error: {e}"),
                },
                Err(e) => eprintln!("rpc error: {e}"),
            }
        }
        Commands::EscrowBalance { account, url } => {
            let payload = foundation_serialization::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "rent.escrow.balance",
                "params": {"id": account},
            });
            let client = http_client::blocking_client();
            match client
                .request(Method::Post, &url)
                .and_then(|builder| builder.json(&payload))
                .and_then(|builder| builder.send())
            {
                Ok(resp) => match resp.json::<Value>() {
                    Ok(v) => println!("{}", v["result"].as_u64().unwrap_or(0)),
                    Err(e) => eprintln!("parse error: {e}"),
                },
                Err(e) => eprintln!("rpc error: {e}"),
            }
        }
        Commands::EscrowRelease { id, amount, url } => {
            let payload = foundation_serialization::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "dex.escrow_release",
                "params": {"id": id, "amount": amount},
            });
            let client = http_client::blocking_client();
            match client
                .request(Method::Post, &url)
                .and_then(|builder| builder.json(&payload))
                .and_then(|builder| builder.send())
            {
                Ok(resp) => match resp.json::<Value>() {
                    Ok(v) => {
                        if let Some(res) = v.get("result") {
                            let proof: PaymentProof =
                                json::from_value(res["proof"].clone()).expect("proof");
                            let root: [u8; 32] =
                                json::from_value(res["root"].clone()).expect("root");
                            let idx = res["idx"].as_u64().unwrap_or(0) as usize;
                            if verify_proof(proof.leaf, idx, &proof.path, root, proof.algo) {
                                println!("released");
                            } else {
                                eprintln!("invalid proof");
                            }
                        } else if let Some(err) = v.get("error") {
                            eprintln!("{}", err);
                        }
                    }
                    Err(e) => eprintln!("parse error: {e}"),
                },
                Err(e) => eprintln!("rpc error: {e}"),
            }
        }
        Commands::BlobPut { file, owner } => {
            let data = std::fs::read(&file).expect("read file");
            let mut pipeline = StoragePipeline::open("blobstore");
            let mut catalog = NodeCatalog::new();
            catalog.register(DummyProvider { id: "local".into() });
            let (_receipt, tx) = pipeline
                .put_object(&data, &owner, &mut catalog)
                .expect("store blob");
            println!("{}", crypto_suite::hex::encode(tx.blob_root));
        }
        Commands::BlobGet { blob_id, out } => {
            let mut arr = [0u8; 32];
            let bytes = decode(&blob_id).expect("blob id hex");
            arr.copy_from_slice(&bytes[..32]);
            let pipeline = StoragePipeline::open("blobstore");
            match pipeline.get_object(&arr) {
                Ok(data) => {
                    std::fs::write(&out, &data).expect("write file");
                    println!("wrote {} bytes", data.len());
                }
                Err(e) => eprintln!("get error: {e}"),
            }
        }
        Commands::Help => {
            print_root_help(&command, &bin);
        }
    }
}
