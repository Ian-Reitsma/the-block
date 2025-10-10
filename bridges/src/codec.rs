use foundation_serialization::json::{Map, Value};

use ledger::{Emission, TokenRegistry};

use crate::{
    header::PowHeader,
    light_client::{Header, Proof},
    relayer::{Relayer, RelayerSet},
    token_bridge::TokenBridge,
    PendingWithdrawal, RelayerBundle, RelayerProof,
};

/// Error raised when encoding or decoding bridge data structures.
#[derive(Debug)]
pub enum Error {
    MissingField(&'static str),
    InvalidType {
        field: &'static str,
        expected: &'static str,
    },
    InvalidValue {
        field: &'static str,
        reason: String,
    },
    Hex {
        field: &'static str,
        source: crypto_suite::hex::Error,
    },
}

impl Error {
    fn missing(field: &'static str) -> Self {
        Self::MissingField(field)
    }

    fn invalid_type(field: &'static str, expected: &'static str) -> Self {
        Self::InvalidType { field, expected }
    }

    fn invalid_value(field: &'static str, reason: impl Into<String>) -> Self {
        Self::InvalidValue {
            field,
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::MissingField(field) => write!(f, "missing field '{field}'"),
            Error::InvalidType { field, expected } => {
                write!(f, "field '{field}' is not {expected}")
            }
            Error::InvalidValue { field, reason } => {
                write!(f, "invalid value for field '{field}': {reason}")
            }
            Error::Hex { field, source } => {
                write!(f, "invalid hex for field '{field}': {source}")
            }
        }
    }
}

impl std::error::Error for Error {}

fn get<'a>(object: &'a Map, field: &'static str) -> Result<&'a Value, Error> {
    object.get(field).ok_or_else(|| Error::missing(field))
}

fn require_object<'a>(value: &'a Value, field: &'static str) -> Result<&'a Map, Error> {
    value
        .as_object()
        .ok_or_else(|| Error::invalid_type(field, "an object"))
}

fn require_array<'a>(value: &'a Value, field: &'static str) -> Result<&'a [Value], Error> {
    value
        .as_array()
        .map(|values| values.as_slice())
        .ok_or_else(|| Error::invalid_type(field, "an array"))
}

fn require_string<'a>(value: &'a Value, field: &'static str) -> Result<&'a str, Error> {
    value
        .as_str()
        .ok_or_else(|| Error::invalid_type(field, "a string"))
}

fn require_u64(value: &Value, field: &'static str) -> Result<u64, Error> {
    value
        .as_u64()
        .ok_or_else(|| Error::invalid_type(field, "an integer"))
}

fn require_bool(value: &Value, field: &'static str) -> Result<bool, Error> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        _ => Err(Error::invalid_type(field, "a boolean")),
    }
}

fn object(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in fields {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn decode_hex_field<const N: usize>(value: &Value, field: &'static str) -> Result<[u8; N], Error> {
    let hex_str = require_string(value, field)?;
    crypto_suite::hex::decode_array::<N>(hex_str).map_err(|source| Error::Hex { field, source })
}

fn encode_hex(bytes: &[u8]) -> Value {
    Value::String(crypto_suite::hex::encode(bytes))
}

fn encode_hex_array(items: &[[u8; 32]]) -> Value {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(Value::String(crypto_suite::hex::encode(item)));
    }
    Value::Array(out)
}

fn decode_hex_vec(value: &Value, field: &'static str) -> Result<Vec<[u8; 32]>, Error> {
    let entries = require_array(value, field)?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let bytes = decode_hex_field::<32>(entry, field)?;
        out.push(bytes);
    }
    Ok(out)
}

fn emission_to_value(emission: &Emission) -> Value {
    match emission {
        Emission::Fixed(amount) => object([
            ("kind", Value::String("fixed".into())),
            ("amount", Value::from(*amount)),
        ]),
        Emission::Linear { initial, rate } => object([
            ("kind", Value::String("linear".into())),
            ("initial", Value::from(*initial)),
            ("rate", Value::from(*rate)),
        ]),
    }
}

fn emission_from_value(value: &Value) -> Result<Emission, Error> {
    let obj = require_object(value, "emission")?;
    let kind = require_string(get(obj, "kind")?, "kind")?;
    match kind {
        "fixed" => {
            let amount = require_u64(get(obj, "amount")?, "amount")?;
            Ok(Emission::Fixed(amount))
        }
        "linear" => {
            let initial = require_u64(get(obj, "initial")?, "initial")?;
            let rate = require_u64(get(obj, "rate")?, "rate")?;
            Ok(Emission::Linear { initial, rate })
        }
        other => Err(Error::invalid_value(
            "kind",
            format!("unknown emission kind '{other}'"),
        )),
    }
}

impl RelayerProof {
    pub fn to_value(&self) -> Value {
        object([
            ("relayer", Value::String(self.relayer.clone())),
            ("commitment", encode_hex(&self.commitment)),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let obj = require_object(value, "relayer_proof")?;
        let relayer = require_string(get(obj, "relayer")?, "relayer")?.to_string();
        let commitment = decode_hex_field::<32>(get(obj, "commitment")?, "commitment")?;
        Ok(Self {
            relayer,
            commitment,
        })
    }
}

impl RelayerBundle {
    pub fn to_value(&self) -> Value {
        let proofs: Vec<Value> = self.proofs.iter().map(RelayerProof::to_value).collect();
        Value::Array(proofs)
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let arr = require_array(value, "relayer_bundle")?;
        let mut proofs = Vec::with_capacity(arr.len());
        for entry in arr {
            proofs.push(RelayerProof::from_value(entry)?);
        }
        Ok(Self::new(proofs))
    }
}

impl PendingWithdrawal {
    pub fn to_value(&self) -> Value {
        let relayers = self
            .relayers
            .iter()
            .map(|r| Value::String(r.clone()))
            .collect();
        object([
            ("user", Value::String(self.user.clone())),
            ("amount", Value::from(self.amount)),
            ("relayers", Value::Array(relayers)),
            ("initiated_at", Value::from(self.initiated_at)),
            ("challenged", Value::Bool(self.challenged)),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let obj = require_object(value, "pending_withdrawal")?;
        let user = require_string(get(obj, "user")?, "user")?.to_string();
        let amount = require_u64(get(obj, "amount")?, "amount")?;
        let relayers_value = get(obj, "relayers")?;
        let relayers_array = require_array(relayers_value, "relayers")?;
        let mut relayers = Vec::with_capacity(relayers_array.len());
        for entry in relayers_array {
            relayers.push(require_string(entry, "relayers")?.to_string());
        }
        let initiated_at = require_u64(get(obj, "initiated_at")?, "initiated_at")?;
        let challenged = require_bool(get(obj, "challenged")?, "challenged")?;
        Ok(Self {
            user,
            amount,
            relayers,
            initiated_at,
            challenged,
        })
    }
}

impl Relayer {
    pub fn to_value(&self) -> Value {
        object([
            ("stake", Value::from(self.stake)),
            ("slashes", Value::from(self.slashes)),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let obj = require_object(value, "relayer")?;
        let stake = require_u64(get(obj, "stake")?, "stake")?;
        let slashes = require_u64(get(obj, "slashes")?, "slashes")?;
        Ok(Self { stake, slashes })
    }
}

impl RelayerSet {
    pub fn to_value(&self) -> Value {
        let mut entries = Vec::new();
        for (id, relayer) in self.snapshot() {
            let mut map = Map::new();
            map.insert("id".to_string(), Value::String(id));
            map.insert("state".to_string(), relayer.to_value());
            entries.push(Value::Object(map));
        }
        Value::Array(entries)
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let arr = require_array(value, "relayer_set")?;
        let mut set = RelayerSet::default();
        for entry in arr {
            let obj = require_object(entry, "relayer_entry")?;
            let id = require_string(get(obj, "id")?, "id")?.to_string();
            let relayer = Relayer::from_value(get(obj, "state")?)?;
            set.insert_state(id, relayer);
        }
        Ok(set)
    }
}

impl Header {
    pub fn to_value(&self) -> Value {
        object([
            ("chain_id", Value::String(self.chain_id.clone())),
            ("height", Value::from(self.height)),
            ("merkle_root", encode_hex(&self.merkle_root)),
            ("signature", encode_hex(&self.signature)),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let obj = require_object(value, "header")?;
        Ok(Self {
            chain_id: require_string(get(obj, "chain_id")?, "chain_id")?.to_string(),
            height: require_u64(get(obj, "height")?, "height")?,
            merkle_root: decode_hex_field::<32>(get(obj, "merkle_root")?, "merkle_root")?,
            signature: decode_hex_field::<32>(get(obj, "signature")?, "signature")?,
        })
    }
}

impl Proof {
    pub fn to_value(&self) -> Value {
        object([
            ("leaf", encode_hex(&self.leaf)),
            ("path", encode_hex_array(&self.path)),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let obj = require_object(value, "proof")?;
        Ok(Self {
            leaf: decode_hex_field::<32>(get(obj, "leaf")?, "leaf")?,
            path: decode_hex_vec(get(obj, "path")?, "path")?,
        })
    }
}

impl PowHeader {
    pub fn to_value(&self) -> Value {
        object([
            ("chain_id", Value::String(self.chain_id.clone())),
            ("height", Value::from(self.height)),
            ("merkle_root", encode_hex(&self.merkle_root)),
            ("signature", encode_hex(&self.signature)),
            ("nonce", Value::from(self.nonce)),
            ("target", Value::from(self.target)),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let obj = require_object(value, "pow_header")?;
        Ok(Self {
            chain_id: require_string(get(obj, "chain_id")?, "chain_id")?.to_string(),
            height: require_u64(get(obj, "height")?, "height")?,
            merkle_root: decode_hex_field::<32>(get(obj, "merkle_root")?, "merkle_root")?,
            signature: decode_hex_field::<32>(get(obj, "signature")?, "signature")?,
            nonce: require_u64(get(obj, "nonce")?, "nonce")?,
            target: require_u64(get(obj, "target")?, "target")?,
        })
    }
}

impl TokenBridge {
    pub fn to_value(&self) -> Value {
        let mut tokens = Vec::new();
        for (symbol, emission) in self.tokens() {
            let mut map = Map::new();
            map.insert("symbol".to_string(), Value::String(symbol));
            map.insert("emission".to_string(), emission_to_value(&emission));
            tokens.push(Value::Object(map));
        }
        object([("tokens", Value::Array(tokens))])
    }

    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let obj = require_object(value, "token_bridge")?;
        let tokens_value = get(obj, "tokens")?;
        let entries = require_array(tokens_value, "tokens")?;
        let mut registry = TokenRegistry::new();
        for entry in entries {
            let token_obj = require_object(entry, "token")?;
            let symbol = require_string(get(token_obj, "symbol")?, "symbol")?.to_string();
            let emission = emission_from_value(get(token_obj, "emission")?)?;
            let _ = registry.register(&symbol, emission);
        }
        Ok(TokenBridge::with_registry(registry))
    }
}
