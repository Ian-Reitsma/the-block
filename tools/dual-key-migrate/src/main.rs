use foundation_serialization::json::{self, Map, Value};
use std::fs::File;
use std::io::{Read, Write};

enum RunError {
    Usage(String),
    Failure(String),
}

struct LegacyAccount {
    address: String,
    ed25519_pub: String,
}

struct DualAccount {
    address: String,
    ed25519_pub: String,
    dilithium_pub: String,
}

fn main() {
    if let Err(err) = run() {
        match err {
            RunError::Usage(msg) => {
                eprintln!("{msg}");
                std::process::exit(2);
            }
            RunError::Failure(msg) => {
                eprintln!("{msg}");
                std::process::exit(1);
            }
        }
    }
}

fn run() -> Result<(), RunError> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err(RunError::Usage(
            "usage: dual-key-migrate <in.json> <out.json>".to_string(),
        ));
    }

    let mut data = String::new();
    File::open(&args[1])
        .map_err(|err| RunError::Failure(format!("failed to open input file: {err}")))?
        .read_to_string(&mut data)
        .map_err(|err| RunError::Failure(format!("failed to read input file: {err}")))?;

    let accounts = parse_legacy_accounts(&data)?;
    let mut out_accounts = Vec::with_capacity(accounts.len());
    for acc in accounts {
        let (pk, _sk) = crypto::dilithium::keypair();
        out_accounts.push(DualAccount {
            address: acc.address,
            ed25519_pub: acc.ed25519_pub,
            dilithium_pub: encode_hex(&pk),
        });
    }

    let rendered = render_accounts(&out_accounts)?;
    File::create(&args[2])
        .map_err(|err| RunError::Failure(format!("failed to create output file: {err}")))?
        .write_all(rendered.as_bytes())
        .map_err(|err| RunError::Failure(format!("failed to write output file: {err}")))?;

    Ok(())
}

fn parse_legacy_accounts(data: &str) -> Result<Vec<LegacyAccount>, RunError> {
    let value = json::value_from_str(data)
        .map_err(|err| RunError::Failure(format!("failed to parse input JSON: {err}")))?;

    let entries = match value {
        Value::Array(entries) => entries,
        other => {
            return Err(RunError::Failure(format!(
                "input JSON must be an array, found {}",
                describe_value(&other)
            )))
        }
    };

    let mut accounts = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let account = LegacyAccount::from_value(entry)
            .map_err(|err| RunError::Failure(format!("invalid account at index {index}: {err}")))?;
        accounts.push(account);
    }

    Ok(accounts)
}

fn render_accounts(accounts: &[DualAccount]) -> Result<String, RunError> {
    let mut values = Vec::with_capacity(accounts.len());
    for account in accounts {
        values.push(account.to_value());
    }

    Ok(json::to_string_value_pretty(&Value::Array(values)))
}

impl LegacyAccount {
    fn from_value(value: &Value) -> Result<Self, String> {
        let map = value
            .as_object()
            .ok_or_else(|| format!("expected object, found {}", describe_value(value)))?;

        let address = expect_string(map, "address")?;
        let ed25519_pub = expect_string(map, "ed25519_pub")?;

        Ok(Self {
            address,
            ed25519_pub,
        })
    }
}

impl DualAccount {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("address".to_string(), Value::String(self.address.clone()));
        map.insert(
            "ed25519_pub".to_string(),
            Value::String(self.ed25519_pub.clone()),
        );
        map.insert(
            "dilithium_pub".to_string(),
            Value::String(self.dilithium_pub.clone()),
        );
        Value::Object(map)
    }
}

fn expect_string(map: &Map, key: &str) -> Result<String, String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing or non-string '{key}' field"))
}

fn describe_value(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(TABLE[(byte >> 4) as usize] as char);
        out.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    out
}
