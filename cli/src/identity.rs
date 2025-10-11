use cli_core::{
    arg::{ArgSpec, OptionSpec},
    command::{Command, CommandBuilder, CommandId},
    parse::Matches,
};
use foundation_serialization::{json, Deserialize, Serialize};
use foundation_unicode::{NormalizationAccuracy, Normalizer};

use crate::rpc::RpcClient;

const DEFAULT_RPC: &str = "http://127.0.0.1:26657";

pub enum IdentityCmd {
    Register {
        handle: String,
        pubkey: String,
        #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
        pq_pubkey: Option<String>,
        sig: String,
        nonce: u64,
        rpc: String,
    },
    Resolve {
        handle: String,
        rpc: String,
    },
    Normalize {
        handle: String,
    },
    Help,
}

impl IdentityCmd {
    pub fn command() -> Command {
        #[allow(unused_mut)]
        let mut register = CommandBuilder::new(
            CommandId("identity.register"),
            "register",
            "Register a handle via JSON-RPC",
        )
        .arg(ArgSpec::Option(
            OptionSpec::new("handle", "handle", "Handle to register").required(true),
        ))
        .arg(ArgSpec::Option(
            OptionSpec::new("pubkey", "pubkey", "Account public key (hex)").required(true),
        ));

        #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
        {
            register = register.arg(ArgSpec::Option(OptionSpec::new(
                "pq-pubkey",
                "pq-pubkey",
                "Optional post-quantum public key (hex)",
            )));
        }

        let register = register
            .arg(ArgSpec::Option(
                OptionSpec::new(
                    "sig",
                    "sig",
                    "Signature over the registration payload (hex)",
                )
                .required(true),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("nonce", "nonce", "Registration nonce").required(true),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("rpc", "rpc", "RPC endpoint").default(DEFAULT_RPC),
            ))
            .build();

        CommandBuilder::new(
            CommandId("identity"),
            "identity",
            "Identity handle utilities",
        )
        .subcommand(register)
        .subcommand(
            CommandBuilder::new(
                CommandId("identity.resolve"),
                "resolve",
                "Resolve a handle to its address",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("handle", "handle", "Handle to resolve").required(true),
            ))
            .arg(ArgSpec::Option(
                OptionSpec::new("rpc", "rpc", "RPC endpoint").default(DEFAULT_RPC),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("identity.normalize"),
                "normalize",
                "Show local normalization results for a handle",
            )
            .arg(ArgSpec::Option(
                OptionSpec::new("handle", "handle", "Handle to inspect").required(true),
            ))
            .build(),
        )
        .subcommand(
            CommandBuilder::new(
                CommandId("identity.help"),
                "help",
                "Show identity command help",
            )
            .build(),
        )
        .build()
    }

    pub fn from_matches(matches: &Matches) -> Result<Self, String> {
        let (name, sub_matches) = matches
            .subcommand()
            .ok_or_else(|| "missing subcommand for 'identity'".to_string())?;

        match name {
            "register" => {
                let handle = required_option(sub_matches, "handle")?;
                let pubkey = required_option(sub_matches, "pubkey")?;
                #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
                let pq_pubkey = sub_matches.get_string("pq-pubkey");
                let sig = required_option(sub_matches, "sig")?;
                let nonce = required_option(sub_matches, "nonce")?
                    .parse::<u64>()
                    .map_err(|_| "invalid nonce".to_string())?;
                let rpc = optional_option(sub_matches, "rpc", DEFAULT_RPC);
                Ok(IdentityCmd::Register {
                    handle,
                    pubkey,
                    #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
                    pq_pubkey,
                    sig,
                    nonce,
                    rpc,
                })
            }
            "resolve" => {
                let handle = required_option(sub_matches, "handle")?;
                let rpc = optional_option(sub_matches, "rpc", DEFAULT_RPC);
                Ok(IdentityCmd::Resolve { handle, rpc })
            }
            "normalize" => {
                let handle = required_option(sub_matches, "handle")?;
                Ok(IdentityCmd::Normalize { handle })
            }
            "help" => Ok(IdentityCmd::Help),
            other => Err(format!("unknown identity command '{other}'")),
        }
    }
}

fn required_option(matches: &Matches, name: &str) -> Result<String, String> {
    matches
        .get_string(name)
        .ok_or_else(|| format!("missing required option '--{name}'"))
}

fn optional_option(matches: &Matches, name: &str, default: &str) -> String {
    matches
        .get_string(name)
        .unwrap_or_else(|| default.to_string())
}

#[derive(Serialize)]
struct RegisterPayload<'a> {
    jsonrpc: &'static str,
    id: u32,
    method: &'static str,
    params: RegisterParams<'a>,
}

#[derive(Serialize)]
struct RegisterParams<'a> {
    handle: &'a str,
    pubkey: &'a str,
    #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pq_pubkey: Option<&'a str>,
    sig: &'a str,
    nonce: u64,
}

#[derive(Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
struct RpcEnvelope<T> {
    result: Option<T>,
    error: Option<RpcErrorBody>,
}

#[derive(Deserialize)]
struct RegisterResult {
    address: String,
    normalized_handle: Option<String>,
    normalization_accuracy: Option<String>,
}

#[derive(Deserialize)]
struct ResolveResult {
    address: Option<String>,
}

pub fn handle(cmd: IdentityCmd) {
    match cmd {
        IdentityCmd::Register {
            handle,
            pubkey,
            #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
            pq_pubkey,
            sig,
            nonce,
            rpc,
        } => {
            if let Err(err) = register_handle(
                &handle,
                &pubkey,
                #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
                pq_pubkey.as_deref(),
                &sig,
                nonce,
                &rpc,
            ) {
                eprintln!("register failed: {err}");
            }
        }
        IdentityCmd::Resolve { handle, rpc } => {
            if let Err(err) = resolve_handle(&handle, &rpc) {
                eprintln!("resolve failed: {err}");
            }
        }
        IdentityCmd::Normalize { handle } => show_local_normalization(&handle),
        IdentityCmd::Help => {
            println!("subcommands: register, resolve, normalize");
        }
    }
}

#[cfg(any(feature = "pq-crypto", feature = "quantum"))]
fn register_handle(
    handle: &str,
    pubkey: &str,
    pq_pubkey: Option<&str>,
    sig: &str,
    nonce: u64,
    rpc: &str,
) -> Result<(), String> {
    register_handle_impl(handle, pubkey, pq_pubkey, sig, nonce, rpc)
}

#[cfg(not(any(feature = "pq-crypto", feature = "quantum")))]
fn register_handle(
    handle: &str,
    pubkey: &str,
    sig: &str,
    nonce: u64,
    rpc: &str,
) -> Result<(), String> {
    register_handle_impl(handle, pubkey, None, sig, nonce, rpc)
}

fn register_handle_impl(
    handle: &str,
    pubkey: &str,
    pq_pubkey: Option<&str>,
    sig: &str,
    nonce: u64,
    rpc: &str,
) -> Result<(), String> {
    let client = RpcClient::from_env();
    #[cfg(not(any(feature = "pq-crypto", feature = "quantum")))]
    let _ = pq_pubkey;
    let payload = RegisterPayload {
        jsonrpc: "2.0",
        id: 1,
        method: "register_handle",
        params: RegisterParams {
            handle,
            pubkey,
            #[cfg(any(feature = "pq-crypto", feature = "quantum"))]
            pq_pubkey,
            sig,
            nonce,
        },
    };
    let response = client
        .call(rpc, &payload)
        .map_err(|err| format!("rpc call failed: {err}"))?
        .json::<RpcEnvelope<RegisterResult>>()
        .map_err(|err| format!("decode response failed: {err}"))?;

    if let Some(error) = response.error {
        return Err(format!("rpc error {}: {}", error.code, error.message));
    }
    let result = response
        .result
        .ok_or_else(|| "rpc response missing result".to_string())?;

    println!("address: {}", result.address);
    let local = Normalizer::default().nfkc(handle);
    println!(
        "local normalized: {} ({})",
        local.as_str(),
        local.accuracy().as_str()
    );
    if let Some(normalized) = result.normalized_handle {
        let accuracy = result
            .normalization_accuracy
            .unwrap_or_else(|| "unknown".to_string());
        println!("node normalized: {} ({})", normalized, accuracy);
        if accuracy == NormalizationAccuracy::Approximate.as_str() {
            eprintln!(
                "warning: remote normalization is approximate; consider adjusting the handle"
            );
        }
    }
    Ok(())
}

fn resolve_handle(handle: &str, rpc: &str) -> Result<(), String> {
    let client = RpcClient::from_env();
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "resolve_handle",
        "params": {"handle": handle},
    });
    let response = client
        .call(rpc, &payload)
        .map_err(|err| format!("rpc call failed: {err}"))?
        .json::<RpcEnvelope<ResolveResult>>()
        .map_err(|err| format!("decode response failed: {err}"))?;
    if let Some(error) = response.error {
        return Err(format!("rpc error {}: {}", error.code, error.message));
    }
    let result = response
        .result
        .ok_or_else(|| "rpc response missing result".to_string())?;
    match result.address {
        Some(addr) => println!("address: {addr}"),
        None => println!("handle not found"),
    }
    Ok(())
}

fn show_local_normalization(handle: &str) {
    let normalized = Normalizer::default().nfkc(handle);
    println!(
        "normalized: {} ({})",
        normalized.as_str(),
        normalized.accuracy().as_str()
    );
}
