use diagnostics::tracing::{debug, warn};
use foundation_serialization::serde::{
    self,
    de::{self, MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq, SerializeStruct},
};
use foundation_serialization::{binary, Deserialize, Serialize};
use state::Proof;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::config::{load_user_config, state_cache_path, LightClientConfig};

const DEFAULT_LAG_THRESHOLD: u64 = 8;
const DEFAULT_MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

#[cfg(feature = "telemetry")]
mod telemetry {
    use foundation_lazy::sync::Lazy;
    use runtime::telemetry::{IntCounter, Registry};

    pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

    fn register_counter(name: &str, help: &str) -> IntCounter {
        let counter = IntCounter::new(name, help).expect("create light-client counter");
        REGISTRY
            .register(Box::new(counter.clone()))
            .expect("register light-client counter");
        counter
    }

    pub static LIGHT_STATE_SNAPSHOT_COMPRESSED_BYTES: Lazy<IntCounter> = Lazy::new(|| {
        register_counter(
            "the_block_light_state_snapshot_compressed_bytes",
            "Total compressed bytes processed for light-client snapshots",
        )
    });

    pub static LIGHT_STATE_SNAPSHOT_DECOMPRESSED_BYTES: Lazy<IntCounter> = Lazy::new(|| {
        register_counter(
            "the_block_light_state_snapshot_decompressed_bytes",
            "Total decompressed bytes applied from light-client snapshots",
        )
    });

    pub static LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
        register_counter(
            "the_block_light_state_snapshot_decompress_errors_total",
            "Total lz77-rle decompression failures for light-client snapshots",
        )
    });
}

#[cfg(feature = "telemetry")]
pub use telemetry::{
    LIGHT_STATE_SNAPSHOT_COMPRESSED_BYTES, LIGHT_STATE_SNAPSHOT_DECOMPRESSED_BYTES,
    LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL, REGISTRY as STATE_STREAM_TELEMETRY_REGISTRY,
};

trait AddressOrd {
    fn address_str(&self) -> &str;
}

struct SortedAccountSeq<'a, T: AddressOrd> {
    accounts: &'a [T],
}

impl<'a, T: AddressOrd> SortedAccountSeq<'a, T> {
    fn new(accounts: &'a [T]) -> Self {
        Self { accounts }
    }
}

impl<'a, T> Serialize for SortedAccountSeq<'a, T>
where
    T: AddressOrd + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut entries: Vec<&T> = self.accounts.iter().collect();
        entries.sort_by(|a, b| a.address_str().cmp(b.address_str()));
        let mut seq = serializer.serialize_seq(Some(entries.len()))?;
        for account in entries {
            seq.serialize_element(account)?;
        }
        seq.end()
    }
}

/// Account-level update carried by a [`StateChunk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountChunk {
    pub address: String,
    pub balance: u64,
    pub account_seq: u64,
    pub proof: Proof,
}

impl AddressOrd for AccountChunk {
    fn address_str(&self) -> &str {
        &self.address
    }
}

impl Serialize for AccountChunk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("AccountChunk", 4)?;
        state.serialize_field("address", &self.address)?;
        state.serialize_field("balance", &self.balance)?;
        state.serialize_field("account_seq", &self.account_seq)?;
        let entries: Vec<([u8; 32], bool)> = self.proof.0.iter().copied().collect();
        state.serialize_field("proof", &entries)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for AccountChunk {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            Address,
            Balance,
            AccountSeq,
            Proof,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("`address`, `balance`, `account_seq`, or `proof`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "address" => Ok(Field::Address),
                            "balance" => Ok(Field::Balance),
                            "account_seq" => Ok(Field::AccountSeq),
                            "proof" => Ok(Field::Proof),
                            _ => Err(E::unknown_field(value, &FIELDS)),
                        }
                    }

                    fn visit_string<E>(self, value: String) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        self.visit_str(&value)
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"address" => Ok(Field::Address),
                            b"balance" => Ok(Field::Balance),
                            b"account_seq" => Ok(Field::AccountSeq),
                            b"proof" => Ok(Field::Proof),
                            _ => {
                                let field = std::str::from_utf8(value).unwrap_or("");
                                Err(E::unknown_field(field, &FIELDS))
                            }
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct AccountChunkVisitor;

        impl<'de> Visitor<'de> for AccountChunkVisitor {
            type Value = AccountChunk;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("account chunk")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let address: String = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("address"))?;
                let balance: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("balance"))?;
                let account_seq: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("account_seq"))?;
                let proof_entries: Vec<([u8; 32], bool)> = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("proof"))?;
                Ok(AccountChunk {
                    address,
                    balance,
                    account_seq,
                    proof: Proof(proof_entries),
                })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut address = None;
                let mut balance = None;
                let mut account_seq = None;
                let mut proof = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Address => {
                            if address.is_some() {
                                return Err(de::Error::duplicate_field("address"));
                            }
                            address = Some(map.next_value()?);
                        }
                        Field::Balance => {
                            if balance.is_some() {
                                return Err(de::Error::duplicate_field("balance"));
                            }
                            balance = Some(map.next_value()?);
                        }
                        Field::AccountSeq => {
                            if account_seq.is_some() {
                                return Err(de::Error::duplicate_field("account_seq"));
                            }
                            account_seq = Some(map.next_value()?);
                        }
                        Field::Proof => {
                            if proof.is_some() {
                                return Err(de::Error::duplicate_field("proof"));
                            }
                            proof = Some(map.next_value::<Vec<([u8; 32], bool)>>()?);
                        }
                    }
                }
                Ok(AccountChunk {
                    address: address.ok_or_else(|| de::Error::missing_field("address"))?,
                    balance: balance.ok_or_else(|| de::Error::missing_field("balance"))?,
                    account_seq: account_seq
                        .ok_or_else(|| de::Error::missing_field("account_seq"))?,
                    proof: Proof(proof.ok_or_else(|| de::Error::missing_field("proof"))?),
                })
            }
        }

        const FIELDS: &[&str] = &["address", "balance", "account_seq", "proof"];
        deserializer.deserialize_struct("AccountChunk", FIELDS, AccountChunkVisitor)
    }
}

/// A chunk of state updates delivered over the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChunk {
    /// Monotonic sequence number.
    pub seq: u64,
    /// Optional latest chain height for lag detection.
    pub tip_height: u64,
    /// Updated account balances keyed by address along with proofs.
    pub accounts: Vec<AccountChunk>,
    /// Merkle root of the accounts in this chunk.
    pub root: [u8; 32],
    /// Indicates if this chunk is a full snapshot compressed with `lz77-rle`.
    pub compressed: bool,
}

impl Serialize for StateChunk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("StateChunk", 5)?;
        state.serialize_field("seq", &self.seq)?;
        state.serialize_field("tip_height", &self.tip_height)?;
        state.serialize_field("accounts", &SortedAccountSeq::new(&self.accounts))?;
        state.serialize_field("root", &self.root)?;
        state.serialize_field("compressed", &self.compressed)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for StateChunk {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            Seq,
            TipHeight,
            Accounts,
            Root,
            Compressed,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter
                            .write_str("`seq`, `tip_height`, `accounts`, `root`, or `compressed`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "seq" => Ok(Field::Seq),
                            "tip_height" => Ok(Field::TipHeight),
                            "accounts" => Ok(Field::Accounts),
                            "root" => Ok(Field::Root),
                            "compressed" => Ok(Field::Compressed),
                            _ => Err(E::unknown_field(value, &FIELDS)),
                        }
                    }

                    fn visit_string<E>(self, value: String) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        self.visit_str(&value)
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"seq" => Ok(Field::Seq),
                            b"tip_height" => Ok(Field::TipHeight),
                            b"accounts" => Ok(Field::Accounts),
                            b"root" => Ok(Field::Root),
                            b"compressed" => Ok(Field::Compressed),
                            _ => {
                                let field = std::str::from_utf8(value).unwrap_or("");
                                Err(E::unknown_field(field, &FIELDS))
                            }
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct StateChunkVisitor;

        impl<'de> Visitor<'de> for StateChunkVisitor {
            type Value = StateChunk;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("state chunk")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let seq_no: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("seq"))?;
                let tip_height: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("tip_height"))?;
                let accounts: Vec<AccountChunk> = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("accounts"))?;
                let root: [u8; 32] = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("root"))?;
                let compressed: bool = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("compressed"))?;
                Ok(StateChunk {
                    seq: seq_no,
                    tip_height,
                    accounts,
                    root,
                    compressed,
                })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut seq_no = None;
                let mut tip_height = None;
                let mut accounts = None;
                let mut root = None;
                let mut compressed = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Seq => {
                            if seq_no.is_some() {
                                return Err(de::Error::duplicate_field("seq"));
                            }
                            seq_no = Some(map.next_value()?);
                        }
                        Field::TipHeight => {
                            if tip_height.is_some() {
                                return Err(de::Error::duplicate_field("tip_height"));
                            }
                            tip_height = Some(map.next_value()?);
                        }
                        Field::Accounts => {
                            if accounts.is_some() {
                                return Err(de::Error::duplicate_field("accounts"));
                            }
                            accounts = Some(map.next_value()?);
                        }
                        Field::Root => {
                            if root.is_some() {
                                return Err(de::Error::duplicate_field("root"));
                            }
                            root = Some(map.next_value()?);
                        }
                        Field::Compressed => {
                            if compressed.is_some() {
                                return Err(de::Error::duplicate_field("compressed"));
                            }
                            compressed = Some(map.next_value()?);
                        }
                    }
                }
                Ok(StateChunk {
                    seq: seq_no.ok_or_else(|| de::Error::missing_field("seq"))?,
                    tip_height: tip_height.ok_or_else(|| de::Error::missing_field("tip_height"))?,
                    accounts: accounts.ok_or_else(|| de::Error::missing_field("accounts"))?,
                    root: root.ok_or_else(|| de::Error::missing_field("root"))?,
                    compressed: compressed.ok_or_else(|| de::Error::missing_field("compressed"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["seq", "tip_height", "accounts", "root", "compressed"];
        deserializer.deserialize_struct("StateChunk", FIELDS, StateChunkVisitor)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CachedAccount {
    balance: u64,
    seq: u64,
}

impl Serialize for CachedAccount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("CachedAccount", 2)?;
        state.serialize_field("balance", &self.balance)?;
        state.serialize_field("seq", &self.seq)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CachedAccount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            Balance,
            Seq,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("`balance` or `seq`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "balance" => Ok(Field::Balance),
                            "seq" => Ok(Field::Seq),
                            _ => Err(E::unknown_field(value, &FIELDS)),
                        }
                    }

                    fn visit_string<E>(self, value: String) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        self.visit_str(&value)
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"balance" => Ok(Field::Balance),
                            b"seq" => Ok(Field::Seq),
                            _ => {
                                let field = std::str::from_utf8(value).unwrap_or("");
                                Err(E::unknown_field(field, &FIELDS))
                            }
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct CachedAccountVisitor;

        impl<'de> Visitor<'de> for CachedAccountVisitor {
            type Value = CachedAccount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("cached account")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let balance: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("balance"))?;
                let seq_no: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("seq"))?;
                Ok(CachedAccount {
                    balance,
                    seq: seq_no,
                })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut balance = None;
                let mut seq_no = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Balance => {
                            if balance.is_some() {
                                return Err(de::Error::duplicate_field("balance"));
                            }
                            balance = Some(map.next_value()?);
                        }
                        Field::Seq => {
                            if seq_no.is_some() {
                                return Err(de::Error::duplicate_field("seq"));
                            }
                            seq_no = Some(map.next_value()?);
                        }
                    }
                }
                Ok(CachedAccount {
                    balance: balance.ok_or_else(|| de::Error::missing_field("balance"))?,
                    seq: seq_no.ok_or_else(|| de::Error::missing_field("seq"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["balance", "seq"];
        deserializer.deserialize_struct("CachedAccount", FIELDS, CachedAccountVisitor)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PersistedState {
    accounts: HashMap<String, CachedAccount>,
    next_seq: u64,
}

struct PersistedStateRef<'a> {
    accounts: &'a HashMap<String, CachedAccount>,
    next_seq: u64,
}

struct SortedAccountMap<'a> {
    entries: Vec<(&'a String, &'a CachedAccount)>,
}

impl<'a> SortedAccountMap<'a> {
    fn new(accounts: &'a HashMap<String, CachedAccount>) -> Self {
        let mut entries: Vec<_> = accounts.iter().collect();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        Self { entries }
    }
}

impl<'a> Serialize for SortedAccountMap<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (address, account) in &self.entries {
            map.serialize_entry(address.as_str(), *account)?;
        }
        map.end()
    }
}

impl Serialize for PersistedState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("PersistedState", 2)?;
        state.serialize_field("accounts", &SortedAccountMap::new(&self.accounts))?;
        state.serialize_field("next_seq", &self.next_seq)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for PersistedState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            Accounts,
            NextSeq,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("`accounts` or `next_seq`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "accounts" => Ok(Field::Accounts),
                            "next_seq" => Ok(Field::NextSeq),
                            _ => Err(E::unknown_field(value, &FIELDS)),
                        }
                    }

                    fn visit_string<E>(self, value: String) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        self.visit_str(&value)
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"accounts" => Ok(Field::Accounts),
                            b"next_seq" => Ok(Field::NextSeq),
                            _ => {
                                let field = std::str::from_utf8(value).unwrap_or("");
                                Err(E::unknown_field(field, &FIELDS))
                            }
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct PersistedStateVisitor;

        impl<'de> Visitor<'de> for PersistedStateVisitor {
            type Value = PersistedState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("persisted state")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let accounts: HashMap<String, CachedAccount> = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("accounts"))?;
                let next_seq: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::missing_field("next_seq"))?;
                Ok(PersistedState { accounts, next_seq })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut accounts = None;
                let mut next_seq = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Accounts => {
                            if accounts.is_some() {
                                return Err(de::Error::duplicate_field("accounts"));
                            }
                            accounts = Some(map.next_value()?);
                        }
                        Field::NextSeq => {
                            if next_seq.is_some() {
                                return Err(de::Error::duplicate_field("next_seq"));
                            }
                            next_seq = Some(map.next_value()?);
                        }
                    }
                }
                Ok(PersistedState {
                    accounts: accounts.ok_or_else(|| de::Error::missing_field("accounts"))?,
                    next_seq: next_seq.ok_or_else(|| de::Error::missing_field("next_seq"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["accounts", "next_seq"];
        deserializer.deserialize_struct("PersistedState", FIELDS, PersistedStateVisitor)
    }
}

impl<'a> Serialize for PersistedStateRef<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("PersistedState", 2)?;
        state.serialize_field("accounts", &SortedAccountMap::new(self.accounts))?;
        state.serialize_field("next_seq", &self.next_seq)?;
        state.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotAccount {
    address: String,
    balance: u64,
    seq: u64,
}

impl AddressOrd for SnapshotAccount {
    fn address_str(&self) -> &str {
        &self.address
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotPayload {
    accounts: Vec<SnapshotAccount>,
    next_seq: u64,
}

impl Serialize for SnapshotAccount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SnapshotAccount", 3)?;
        state.serialize_field("address", &self.address)?;
        state.serialize_field("balance", &self.balance)?;
        state.serialize_field("seq", &self.seq)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for SnapshotAccount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            Address,
            Balance,
            Seq,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("`address`, `balance`, or `seq`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "address" => Ok(Field::Address),
                            "balance" => Ok(Field::Balance),
                            "seq" => Ok(Field::Seq),
                            _ => Err(E::unknown_field(value, &FIELDS)),
                        }
                    }

                    fn visit_string<E>(self, value: String) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        self.visit_str(&value)
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"address" => Ok(Field::Address),
                            b"balance" => Ok(Field::Balance),
                            b"seq" => Ok(Field::Seq),
                            _ => {
                                let field = std::str::from_utf8(value).unwrap_or("");
                                Err(E::unknown_field(field, &FIELDS))
                            }
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct SnapshotAccountVisitor;

        impl<'de> Visitor<'de> for SnapshotAccountVisitor {
            type Value = SnapshotAccount;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("snapshot account")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let address = seq
                    .next_element::<String>()?
                    .ok_or_else(|| de::Error::missing_field("address"))?;
                let balance = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| de::Error::missing_field("balance"))?;
                let seq_no = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| de::Error::missing_field("seq"))?;
                Ok(SnapshotAccount {
                    address,
                    balance,
                    seq: seq_no,
                })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut address = None;
                let mut balance = None;
                let mut seq_no = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Address => {
                            if address.is_some() {
                                return Err(de::Error::duplicate_field("address"));
                            }
                            address = Some(map.next_value()?);
                        }
                        Field::Balance => {
                            if balance.is_some() {
                                return Err(de::Error::duplicate_field("balance"));
                            }
                            balance = Some(map.next_value()?);
                        }
                        Field::Seq => {
                            if seq_no.is_some() {
                                return Err(de::Error::duplicate_field("seq"));
                            }
                            seq_no = Some(map.next_value()?);
                        }
                    }
                }
                Ok(SnapshotAccount {
                    address: address.ok_or_else(|| de::Error::missing_field("address"))?,
                    balance: balance.ok_or_else(|| de::Error::missing_field("balance"))?,
                    seq: seq_no.ok_or_else(|| de::Error::missing_field("seq"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["address", "balance", "seq"];
        deserializer.deserialize_struct("SnapshotAccount", FIELDS, SnapshotAccountVisitor)
    }
}

impl Serialize for SnapshotPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SnapshotPayload", 2)?;
        state.serialize_field("accounts", &SortedAccountSeq::new(&self.accounts))?;
        state.serialize_field("next_seq", &self.next_seq)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for SnapshotPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            Accounts,
            NextSeq,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter<'_>,
                    ) -> std::fmt::Result {
                        formatter.write_str("`accounts` or `next_seq`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "accounts" => Ok(Field::Accounts),
                            "next_seq" => Ok(Field::NextSeq),
                            _ => Err(E::unknown_field(value, &FIELDS)),
                        }
                    }

                    fn visit_string<E>(self, value: String) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        self.visit_str(&value)
                    }

                    fn visit_bytes<E>(self, value: &[u8]) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            b"accounts" => Ok(Field::Accounts),
                            b"next_seq" => Ok(Field::NextSeq),
                            _ => {
                                let field = std::str::from_utf8(value).unwrap_or("");
                                Err(E::unknown_field(field, &FIELDS))
                            }
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct SnapshotPayloadVisitor;

        impl<'de> Visitor<'de> for SnapshotPayloadVisitor {
            type Value = SnapshotPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("snapshot payload")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let accounts = seq
                    .next_element::<Vec<SnapshotAccount>>()?
                    .ok_or_else(|| de::Error::missing_field("accounts"))?;
                let next_seq = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| de::Error::missing_field("next_seq"))?;
                Ok(SnapshotPayload { accounts, next_seq })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut accounts = None;
                let mut next_seq = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Accounts => {
                            if accounts.is_some() {
                                return Err(de::Error::duplicate_field("accounts"));
                            }
                            accounts = Some(map.next_value()?);
                        }
                        Field::NextSeq => {
                            if next_seq.is_some() {
                                return Err(de::Error::duplicate_field("next_seq"));
                            }
                            next_seq = Some(map.next_value()?);
                        }
                    }
                }
                Ok(SnapshotPayload {
                    accounts: accounts.ok_or_else(|| de::Error::missing_field("accounts"))?,
                    next_seq: next_seq.ok_or_else(|| de::Error::missing_field("next_seq"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["accounts", "next_seq"];
        deserializer.deserialize_struct("SnapshotPayload", FIELDS, SnapshotPayloadVisitor)
    }
}

/// Callback invoked when the client detects a sequence gap.
pub type GapCallback = Box<dyn FnMut(u64, u64) -> Result<Vec<StateChunk>, StateStreamError> + Send>;

/// Client-side helper maintaining a rolling cache of account state.
pub struct StateStream {
    cache: HashMap<String, CachedAccount>,
    next_seq: u64,
    lag_threshold: u64,
    gap_fetcher: Option<GapCallback>,
    persist_path: Option<PathBuf>,
    max_snapshot_bytes: usize,
}

/// Builder for [`StateStream`].
pub struct StateStreamBuilder {
    cache_path: Option<PathBuf>,
    lag_threshold: u64,
    max_snapshot_bytes: usize,
    gap_fetcher: Option<GapCallback>,
}

#[derive(Debug, Error)]
pub enum StateStreamError {
    #[error("sequence mismatch: expected {expected}, received {received}")]
    SequenceMismatch { expected: u64, received: u64 },
    #[error("invalid proof for address {address}")]
    InvalidProof { address: String },
    #[error("stale update for {address}: cached sequence {cached_seq}, received {update_seq}")]
    StaleAccountUpdate {
        address: String,
        cached_seq: u64,
        update_seq: u64,
    },
    #[error("gap detected: expected {expected}, received {received}")]
    GapDetected { expected: u64, received: u64 },
    #[error("gap recovery incomplete: expected {expected}, recovered {recovered}")]
    GapRecoveryFailed { expected: u64, recovered: u64 },
    #[error("snapshot exceeds max size (size={size}, limit={limit})")]
    SnapshotTooLarge { size: usize, limit: usize },
    #[error("failed to decode snapshot payload: {0}")]
    SnapshotDecode(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StateStreamBuilder {
    pub fn new() -> Self {
        Self {
            cache_path: None,
            lag_threshold: DEFAULT_LAG_THRESHOLD,
            max_snapshot_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
            gap_fetcher: None,
        }
    }

    pub fn cache_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.cache_path = Some(path.into());
        self
    }

    pub fn lag_threshold(mut self, threshold: u64) -> Self {
        self.lag_threshold = threshold;
        self
    }

    pub fn max_snapshot_bytes(mut self, bytes: usize) -> Self {
        self.max_snapshot_bytes = bytes;
        self
    }

    pub fn gap_fetcher<F>(mut self, fetcher: F) -> Self
    where
        F: FnMut(u64, u64) -> Result<Vec<StateChunk>, StateStreamError> + Send + 'static,
    {
        self.gap_fetcher = Some(Box::new(fetcher));
        self
    }

    pub fn build(self) -> StateStream {
        StateStream::from_builder(self)
    }
}

impl Default for StateStreamBuilder {
    fn default() -> Self {
        StateStreamBuilder::new()
    }
}

impl StateStream {
    /// Create a new stream using persisted state (if any) and configuration defaults.
    pub fn new() -> Self {
        let config = load_user_config().unwrap_or_else(|err| {
            warn!("failed to load light client config: {err}");
            LightClientConfig::default()
        });
        Self::from_config(&config)
    }

    /// Create a new stream deriving limits from the provided configuration.
    pub fn from_config(config: &LightClientConfig) -> Self {
        let mut builder =
            StateStreamBuilder::new().max_snapshot_bytes(config.snapshot_limit_bytes());
        if let Some(path) = state_cache_path() {
            builder = builder.cache_path(path);
        }
        builder.build()
    }

    pub fn builder() -> StateStreamBuilder {
        StateStreamBuilder::new()
    }

    fn from_builder(builder: StateStreamBuilder) -> Self {
        let mut cache = HashMap::new();
        let mut next_seq = 0u64;
        if let Some(path) = builder.cache_path.as_ref() {
            match Self::load_persisted(path) {
                Ok(Some(state)) => {
                    cache = state.accounts;
                    next_seq = state.next_seq;
                }
                Ok(None) => {}
                Err(err) => warn!("failed to load persisted light-client cache: {err}"),
            }
        }
        Self {
            cache,
            next_seq,
            lag_threshold: if builder.lag_threshold == 0 {
                DEFAULT_LAG_THRESHOLD
            } else {
                builder.lag_threshold
            },
            gap_fetcher: builder.gap_fetcher,
            persist_path: builder.cache_path,
            max_snapshot_bytes: if builder.max_snapshot_bytes == 0 {
                DEFAULT_MAX_SNAPSHOT_BYTES
            } else {
                builder.max_snapshot_bytes
            },
        }
    }

    fn load_persisted(path: &Path) -> Result<Option<PersistedState>, StateStreamError> {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        match binary::decode::<PersistedState>(&bytes) {
            Ok(state) => Ok(Some(state)),
            Err(_) => {
                if let Ok(legacy) = binary::decode::<HashMap<String, u64>>(&bytes) {
                    let accounts = legacy
                        .into_iter()
                        .map(|(addr, bal)| {
                            (
                                addr,
                                CachedAccount {
                                    balance: bal,
                                    seq: 0,
                                },
                            )
                        })
                        .collect();
                    Ok(Some(PersistedState {
                        accounts,
                        next_seq: 0,
                    }))
                } else {
                    Err(StateStreamError::Serialization(
                        "unable to decode persisted cache".to_string(),
                    ))
                }
            }
        }
    }

    fn persist_state(&self) -> Result<(), StateStreamError> {
        if let Some(path) = &self.persist_path {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let tmp_path = path.with_extension("tmp");
            let state = PersistedStateRef {
                accounts: &self.cache,
                next_seq: self.next_seq,
            };
            let bytes = binary::encode(&state)
                .map_err(|err| StateStreamError::Serialization(err.to_string()))?;
            fs::write(&tmp_path, &bytes)?;
            if let Err(err) = fs::rename(&tmp_path, path) {
                let _ = fs::remove_file(&tmp_path);
                return Err(StateStreamError::Io(err));
            }
        }
        Ok(())
    }

    pub fn set_gap_fetcher<F>(&mut self, fetcher: F)
    where
        F: FnMut(u64, u64) -> Result<Vec<StateChunk>, StateStreamError> + Send + 'static,
    {
        self.gap_fetcher = Some(Box::new(fetcher));
    }

    pub fn lag_threshold(&self) -> u64 {
        self.lag_threshold
    }

    pub fn set_lag_threshold(&mut self, threshold: u64) {
        self.lag_threshold = if threshold == 0 {
            DEFAULT_LAG_THRESHOLD
        } else {
            threshold
        };
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Apply an incremental chunk of updates. Returns an error when validation fails.
    pub fn apply_chunk(&mut self, chunk: StateChunk) -> Result<(), StateStreamError> {
        if chunk.seq > self.next_seq {
            self.recover_gap(chunk.seq)?;
        }
        if chunk.seq < self.next_seq {
            return Err(StateStreamError::SequenceMismatch {
                expected: self.next_seq,
                received: chunk.seq,
            });
        }
        self.apply_chunk_inner(chunk)
    }

    fn recover_gap(&mut self, target_seq: u64) -> Result<(), StateStreamError> {
        if self.gap_fetcher.is_none() {
            return Err(StateStreamError::GapDetected {
                expected: self.next_seq,
                received: target_seq,
            });
        }
        while self.next_seq < target_seq {
            let missing = {
                // Drop the mutable borrow of the callback before applying the chunks so
                // we can borrow `self` mutably again.
                let fetcher = self
                    .gap_fetcher
                    .as_mut()
                    .expect("gap fetcher present while recovering");
                fetcher(self.next_seq, target_seq)?
            };
            if missing.is_empty() {
                break;
            }
            for chunk in missing {
                if chunk.seq != self.next_seq {
                    return Err(StateStreamError::SequenceMismatch {
                        expected: self.next_seq,
                        received: chunk.seq,
                    });
                }
                self.apply_chunk_inner(chunk)?;
            }
        }
        if self.next_seq < target_seq {
            return Err(StateStreamError::GapRecoveryFailed {
                expected: target_seq,
                recovered: self.next_seq,
            });
        }
        Ok(())
    }

    fn apply_chunk_inner(&mut self, chunk: StateChunk) -> Result<(), StateStreamError> {
        if chunk.seq != self.next_seq {
            return Err(StateStreamError::SequenceMismatch {
                expected: self.next_seq,
                received: chunk.seq,
            });
        }
        let root = chunk.root;
        let accounts = chunk.accounts;
        let mut previous: HashMap<String, Option<CachedAccount>> = HashMap::new();
        for account in &accounts {
            previous
                .entry(account.address.clone())
                .or_insert_with(|| self.cache.get(&account.address).cloned());
            let value = account_state_value(account.balance, account.account_seq);
            if !state::MerkleTrie::verify_proof(
                root,
                account.address.as_bytes(),
                &value,
                &account.proof,
            ) {
                return Err(StateStreamError::InvalidProof {
                    address: account.address.clone(),
                });
            }
            if let Some(existing) = self.cache.get(&account.address) {
                if account.account_seq < existing.seq {
                    return Err(StateStreamError::StaleAccountUpdate {
                        address: account.address.clone(),
                        cached_seq: existing.seq,
                        update_seq: account.account_seq,
                    });
                }
            }
        }
        for account in accounts {
            self.cache.insert(
                account.address,
                CachedAccount {
                    balance: account.balance,
                    seq: account.account_seq,
                },
            );
        }
        let prev_next_seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        if let Err(err) = self.persist_state() {
            self.next_seq = prev_next_seq;
            for (address, prior) in previous {
                match prior {
                    Some(entry) => {
                        self.cache.insert(address, entry);
                    }
                    None => {
                        self.cache.remove(&address);
                    }
                }
            }
            return Err(err);
        }
        Ok(())
    }

    /// Apply a full snapshot, optionally compressed with `lz77-rle`.
    pub fn apply_snapshot(
        &mut self,
        data: &[u8],
        compressed: bool,
    ) -> Result<(), StateStreamError> {
        if data.len() > self.max_snapshot_bytes {
            return Err(StateStreamError::SnapshotTooLarge {
                size: data.len(),
                limit: self.max_snapshot_bytes,
            });
        }
        let bytes = if compressed {
            #[cfg(feature = "telemetry")]
            LIGHT_STATE_SNAPSHOT_COMPRESSED_BYTES.inc_by(data.len() as u64);
            match coding::compressor_for("lz77-rle", 4) {
                Ok(compressor) => match compressor.decompress(data) {
                    Ok(decoded) => decoded,
                    Err(err) => {
                        #[cfg(feature = "telemetry")]
                        LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL.inc();
                        return Err(StateStreamError::SnapshotDecode(err.to_string()));
                    }
                },
                Err(err) => {
                    #[cfg(feature = "telemetry")]
                    LIGHT_STATE_SNAPSHOT_DECOMPRESS_ERRORS_TOTAL.inc();
                    return Err(StateStreamError::SnapshotDecode(err.to_string()));
                }
            }
        } else {
            data.to_vec()
        };
        if bytes.len() > self.max_snapshot_bytes {
            return Err(StateStreamError::SnapshotTooLarge {
                size: bytes.len(),
                limit: self.max_snapshot_bytes,
            });
        }
        #[cfg(feature = "telemetry")]
        LIGHT_STATE_SNAPSHOT_DECOMPRESSED_BYTES.inc_by(bytes.len() as u64);
        let payload = Self::decode_snapshot(&bytes)?;
        let mut cache = HashMap::with_capacity(payload.accounts.len());
        for account in payload.accounts {
            cache.insert(
                account.address,
                CachedAccount {
                    balance: account.balance,
                    seq: account.seq,
                },
            );
        }
        let previous_cache = std::mem::replace(&mut self.cache, cache);
        let previous_seq = std::mem::replace(&mut self.next_seq, payload.next_seq);
        if let Err(err) = self.persist_state() {
            self.cache = previous_cache;
            self.next_seq = previous_seq;
            return Err(err);
        }
        debug!(
            snapshot_accounts = self.cache.len(),
            next_seq = self.next_seq,
            "applied state snapshot"
        );
        Ok(())
    }

    fn decode_snapshot(bytes: &[u8]) -> Result<SnapshotPayload, StateStreamError> {
        match binary::decode::<SnapshotPayload>(bytes) {
            Ok(snapshot) => Ok(snapshot),
            Err(_) => {
                if let Ok(legacy) = binary::decode::<HashMap<String, u64>>(bytes) {
                    let accounts = legacy
                        .into_iter()
                        .map(|(address, balance)| SnapshotAccount {
                            address,
                            balance,
                            seq: 0,
                        })
                        .collect();
                    Ok(SnapshotPayload {
                        accounts,
                        next_seq: 0,
                    })
                } else {
                    Err(StateStreamError::SnapshotDecode(
                        "unable to decode snapshot payload".to_string(),
                    ))
                }
            }
        }
    }

    /// Returns true if the client is behind the provided chain height by more
    /// than the configured lag threshold.
    pub fn lagging(&self, tip_height: u64) -> bool {
        let lag = tip_height.saturating_sub(self.next_seq);
        let lagging = lag > self.lag_threshold;
        if lagging {
            warn!(
                lag_blocks = lag,
                lag_threshold = self.lag_threshold,
                next_seq = self.next_seq,
                tip_height,
                "light-client stream is lagging"
            );
        }
        lagging
    }

    pub fn cached_balance(&self, address: &str) -> Option<(u64, u64)> {
        self.cache
            .get(address)
            .map(|entry| (entry.balance, entry.seq))
    }
}

/// Encode an account balance/sequence pair for Merkle verification.
pub fn account_state_value(balance: u64, seq: u64) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&balance.to_le_bytes());
    bytes[8..].copy_from_slice(&seq.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERSISTED_STATE_FIXTURE: &[u8] = &[
        2, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 97, 99, 99, 111, 117, 110, 116, 115, 2, 0,
        0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 97, 108, 105, 99, 101, 2, 0, 0, 0, 0, 0, 0, 0, 7,
        0, 0, 0, 0, 0, 0, 0, 98, 97, 108, 97, 110, 99, 101, 232, 3, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0,
        0, 0, 0, 0, 115, 101, 113, 7, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 98, 111, 98, 2,
        0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 98, 97, 108, 97, 110, 99, 101, 196, 9, 0, 0,
        0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 115, 101, 113, 11, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0,
        0, 0, 0, 110, 101, 120, 116, 95, 115, 101, 113, 42, 0, 0, 0, 0, 0, 0, 0,
    ];

    const SNAPSHOT_FIXTURE: &[u8] = &[
        2, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 97, 99, 99, 111, 117, 110, 116, 115, 3, 0,
        0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 97, 100, 100, 114, 101,
        115, 115, 5, 0, 0, 0, 0, 0, 0, 0, 97, 108, 105, 99, 101, 7, 0, 0, 0, 0, 0, 0, 0, 98, 97,
        108, 97, 110, 99, 101, 232, 3, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 115, 101, 113, 7,
        0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 97, 100, 100, 114,
        101, 115, 115, 3, 0, 0, 0, 0, 0, 0, 0, 98, 111, 98, 7, 0, 0, 0, 0, 0, 0, 0, 98, 97, 108,
        97, 110, 99, 101, 196, 9, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 115, 101, 113, 11, 0,
        0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 97, 100, 100, 114, 101,
        115, 115, 5, 0, 0, 0, 0, 0, 0, 0, 99, 97, 114, 111, 108, 7, 0, 0, 0, 0, 0, 0, 0, 98, 97,
        108, 97, 110, 99, 101, 136, 19, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 115, 101, 113, 3,
        0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 110, 101, 120, 116, 95, 115, 101, 113, 77, 0,
        0, 0, 0, 0, 0, 0,
    ];

    fn sample_state() -> PersistedState {
        let mut accounts = HashMap::new();
        accounts.insert(
            "alice".to_string(),
            CachedAccount {
                balance: 1_000,
                seq: 7,
            },
        );
        accounts.insert(
            "bob".to_string(),
            CachedAccount {
                balance: 2_500,
                seq: 11,
            },
        );
        PersistedState {
            accounts,
            next_seq: 42,
        }
    }

    fn with_first_party_only_env<R>(value: Option<&str>, f: impl FnOnce() -> R) -> R {
        static GUARD: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let lock = GUARD
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("env guard");

        let original = std::env::var("FIRST_PARTY_ONLY").ok();
        match value {
            Some(v) => std::env::set_var("FIRST_PARTY_ONLY", v),
            None => std::env::remove_var("FIRST_PARTY_ONLY"),
        }

        let result = f();

        match original {
            Some(v) => std::env::set_var("FIRST_PARTY_ONLY", v),
            None => std::env::remove_var("FIRST_PARTY_ONLY"),
        }

        drop(lock);
        result
    }

    #[test]
    fn persisted_state_roundtrip_matches_fixture() {
        let state = sample_state();
        let encoded = binary::encode(&state).expect("encode state");
        if PERSISTED_STATE_FIXTURE.is_empty() {
            panic!("fixture pending: {:?}", encoded);
        }

        assert_eq!(encoded, PERSISTED_STATE_FIXTURE);

        let decoded: PersistedState =
            binary::decode(PERSISTED_STATE_FIXTURE).expect("decode state");
        assert_eq!(decoded.next_seq, state.next_seq);
        assert_eq!(decoded.accounts.len(), state.accounts.len());
        for (key, account) in &state.accounts {
            let decoded_account = decoded.accounts.get(key).expect("account present");
            assert_eq!(decoded_account.balance, account.balance);
            assert_eq!(decoded_account.seq, account.seq);
        }
    }

    #[test]
    fn persisted_state_ref_encodes_identically() {
        let state = sample_state();
        let state_ref = PersistedStateRef {
            accounts: &state.accounts,
            next_seq: state.next_seq,
        };
        let ref_bytes = binary::encode(&state_ref).expect("encode ref");
        let owned_bytes = binary::encode(&state).expect("encode owned");
        assert_eq!(ref_bytes, owned_bytes);
        if !PERSISTED_STATE_FIXTURE.is_empty() {
            assert_eq!(ref_bytes, PERSISTED_STATE_FIXTURE);
        }
    }

    fn sample_snapshot() -> SnapshotPayload {
        SnapshotPayload {
            accounts: vec![
                SnapshotAccount {
                    address: "carol".to_string(),
                    balance: 5_000,
                    seq: 3,
                },
                SnapshotAccount {
                    address: "alice".to_string(),
                    balance: 1_000,
                    seq: 7,
                },
                SnapshotAccount {
                    address: "bob".to_string(),
                    balance: 2_500,
                    seq: 11,
                },
            ],
            next_seq: 77,
        }
    }

    #[test]
    fn snapshot_fixture_matches_deterministic_encoding() {
        let snapshot = sample_snapshot();
        let encoded = binary::encode(&snapshot).expect("encode snapshot");
        if SNAPSHOT_FIXTURE.is_empty() {
            panic!("snapshot fixture pending: {:?}", encoded);
        }
        assert_eq!(encoded, SNAPSHOT_FIXTURE);

        let decoded: SnapshotPayload =
            binary::decode(SNAPSHOT_FIXTURE).expect("decode snapshot fixture");
        assert_eq!(decoded.next_seq, snapshot.next_seq);
        assert_eq!(decoded.accounts.len(), snapshot.accounts.len());

        let addresses: Vec<&str> = decoded
            .accounts
            .iter()
            .map(|account| account.address.as_str())
            .collect();
        let mut sorted = addresses.clone();
        sorted.sort_unstable();
        assert_eq!(addresses, sorted);
    }

    #[test]
    fn snapshot_roundtrip_respects_first_party_only_flag() {
        let snapshot = sample_snapshot();

        for flag in [Some("1"), Some("0"), None] {
            with_first_party_only_env(flag, || {
                let encoded = binary::encode(&snapshot).expect("encode snapshot");
                let decoded: SnapshotPayload = binary::decode(&encoded).expect("decode snapshot");
                assert_eq!(decoded.next_seq, snapshot.next_seq);
                assert_eq!(decoded.accounts.len(), snapshot.accounts.len());
            });
        }
    }

    fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        fn helper<T: Clone>(items: &mut Vec<T>, start: usize, acc: &mut Vec<Vec<T>>) {
            if start == items.len() {
                acc.push(items.clone());
                return;
            }
            for index in start..items.len() {
                items.swap(start, index);
                helper(items, start + 1, acc);
                items.swap(start, index);
            }
        }

        let mut working = items.to_vec();
        let mut acc = Vec::new();
        helper(&mut working, 0, &mut acc);
        acc
    }

    #[test]
    fn snapshot_serialization_orders_accounts() {
        let snapshot = sample_snapshot();
        let expected = binary::encode(&snapshot).expect("encode canonical snapshot");

        let accounts = snapshot.accounts.clone();
        for perm in permutations(&accounts) {
            let payload = SnapshotPayload {
                accounts: perm.clone(),
                next_seq: snapshot.next_seq,
            };
            let encoded = binary::encode(&payload).expect("encode permuted snapshot");
            assert_eq!(encoded, expected);
        }
    }

    #[test]
    fn persisted_state_roundtrip_respects_first_party_only_flag() {
        let state = sample_state();

        for flag in [Some("1"), Some("0"), None] {
            with_first_party_only_env(flag, || {
                let encoded = binary::encode(&state).expect("encode state with flag");
                let decoded: PersistedState =
                    binary::decode(&encoded).expect("decode state with flag");
                assert_eq!(decoded.next_seq, state.next_seq);
                assert_eq!(decoded.accounts.len(), state.accounts.len());
            });
        }
    }

    #[derive(Debug)]
    struct PersistedStateWithVec {
        accounts: Vec<(String, CachedAccount)>,
        next_seq: u64,
    }

    struct AccountsVec(Vec<(String, CachedAccount)>);

    impl<'de> Deserialize<'de> for AccountsVec {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            struct AccountsVisitor;

            impl<'de> Visitor<'de> for AccountsVisitor {
                type Value = AccountsVec;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("map of account entries")
                }

                fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    let mut entries = Vec::new();
                    while let Some((address, account)) =
                        map.next_entry::<String, CachedAccount>()?
                    {
                        entries.push((address, account));
                    }
                    Ok(AccountsVec(entries))
                }
            }

            deserializer.deserialize_map(AccountsVisitor)
        }
    }

    impl<'de> Deserialize<'de> for PersistedStateWithVec {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            enum Field {
                Accounts,
                NextSeq,
            }

            impl<'de> Deserialize<'de> for Field {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: serde::Deserializer<'de>,
                {
                    struct FieldVisitor;

                    impl<'de> Visitor<'de> for FieldVisitor {
                        type Value = Field;

                        fn expecting(
                            &self,
                            formatter: &mut std::fmt::Formatter<'_>,
                        ) -> std::fmt::Result {
                            formatter.write_str("`accounts` or `next_seq`")
                        }

                        fn visit_str<E>(self, value: &str) -> Result<Field, E>
                        where
                            E: de::Error,
                        {
                            match value {
                                "accounts" => Ok(Field::Accounts),
                                "next_seq" => Ok(Field::NextSeq),
                                _ => Err(E::unknown_field(value, &FIELDS)),
                            }
                        }

                        fn visit_string<E>(self, value: String) -> Result<Field, E>
                        where
                            E: de::Error,
                        {
                            self.visit_str(&value)
                        }

                        fn visit_bytes<E>(self, value: &[u8]) -> Result<Field, E>
                        where
                            E: de::Error,
                        {
                            match value {
                                b"accounts" => Ok(Field::Accounts),
                                b"next_seq" => Ok(Field::NextSeq),
                                _ => {
                                    let field = std::str::from_utf8(value).unwrap_or("");
                                    Err(E::unknown_field(field, &FIELDS))
                                }
                            }
                        }
                    }

                    deserializer.deserialize_identifier(FieldVisitor)
                }
            }

            struct PersistedStateWithVecVisitor;

            impl<'de> Visitor<'de> for PersistedStateWithVecVisitor {
                type Value = PersistedStateWithVec;

                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("persisted state with ordered accounts")
                }

                fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    let AccountsVec(accounts) = seq
                        .next_element::<AccountsVec>()?
                        .ok_or_else(|| de::Error::missing_field("accounts"))?;
                    let next_seq = seq
                        .next_element::<u64>()?
                        .ok_or_else(|| de::Error::missing_field("next_seq"))?;
                    Ok(PersistedStateWithVec { accounts, next_seq })
                }

                fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    let mut accounts = None;
                    let mut next_seq = None;
                    while let Some(key) = map.next_key::<Field>()? {
                        match key {
                            Field::Accounts => {
                                if accounts.is_some() {
                                    return Err(de::Error::duplicate_field("accounts"));
                                }
                                let AccountsVec(entries) = map.next_value::<AccountsVec>()?;
                                accounts = Some(entries);
                            }
                            Field::NextSeq => {
                                if next_seq.is_some() {
                                    return Err(de::Error::duplicate_field("next_seq"));
                                }
                                next_seq = Some(map.next_value()?);
                            }
                        }
                    }
                    Ok(PersistedStateWithVec {
                        accounts: accounts.ok_or_else(|| de::Error::missing_field("accounts"))?,
                        next_seq: next_seq.ok_or_else(|| de::Error::missing_field("next_seq"))?,
                    })
                }
            }

            const FIELDS: &[&str] = &["accounts", "next_seq"];
            deserializer.deserialize_struct(
                "PersistedStateWithVec",
                FIELDS,
                PersistedStateWithVecVisitor,
            )
        }
    }

    #[test]
    fn persisted_state_serialization_orders_accounts() {
        let mut accounts = HashMap::new();
        accounts.insert(
            "carol".to_string(),
            CachedAccount {
                balance: 3_000,
                seq: 5,
            },
        );
        accounts.insert(
            "alice".to_string(),
            CachedAccount {
                balance: 1_000,
                seq: 7,
            },
        );
        accounts.insert(
            "bob".to_string(),
            CachedAccount {
                balance: 2_500,
                seq: 11,
            },
        );

        let state = PersistedState {
            accounts,
            next_seq: 99,
        };

        let encoded_state = binary::encode(&state).expect("encode unordered state");
        let decoded: PersistedStateWithVec =
            binary::decode(&encoded_state).expect("decode ordered view");

        assert_eq!(decoded.next_seq, state.next_seq);

        let addresses: Vec<&str> = decoded
            .accounts
            .iter()
            .map(|(address, _)| address.as_str())
            .collect();
        let mut sorted = addresses.clone();
        sorted.sort_unstable();
        assert_eq!(addresses, sorted);
    }
}

mod proof_serde {
    use super::*;
    use foundation_serialization::serde::{
        de::{SeqAccess, Visitor},
        ser::SerializeSeq,
        Deserializer, Serializer,
    };
    use std::fmt;

    #[allow(dead_code)]
    pub fn serialize<S>(proof: &Proof, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(proof.0.len()))?;
        for (hash, is_left) in &proof.0 {
            seq.serialize_element(&(*hash, *is_left))?;
        }
        seq.end()
    }

    #[allow(dead_code)]
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Proof, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ProofVisitor;

        impl<'de> Visitor<'de> for ProofVisitor {
            type Value = Proof;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a Merkle proof as a sequence of (hash, is_left)")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((hash, is_left)) = seq.next_element::<([u8; 32], bool)>()? {
                    entries.push((hash, is_left));
                }
                Ok(Proof(entries))
            }
        }

        deserializer.deserialize_seq(ProofVisitor)
    }
}
