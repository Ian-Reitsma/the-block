use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use foundation_serialization::serde::{
    self,
    de::{self, MapAccess, SeqAccess, Visitor},
    ser::{SerializeSeq, SerializeStruct},
};
use foundation_serialization::{binary, Deserialize, Serialize};
use light_client::{
    account_state_value, AccountChunk, StateChunk, StateStream, StateStreamBuilder,
    StateStreamError,
};
use rand::{rngs::StdRng, seq::SliceRandom, Rng};
use state::MerkleTrie;
use sys::tempfile::tempdir;

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

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

fn build_chunk(seq: u64, tip_height: u64, entries: &[(&str, u64, u64)]) -> StateChunk {
    let mut trie = MerkleTrie::new();
    for (address, balance, account_seq) in entries.iter().copied() {
        let value = account_state_value(balance, account_seq);
        trie.insert(address.as_bytes(), &value);
    }
    let root = trie.root_hash();
    let accounts = entries
        .iter()
        .map(|(address, balance, account_seq)| AccountChunk {
            address: (*address).to_string(),
            balance: *balance,
            account_seq: *account_seq,
            proof: trie
                .prove(address.as_bytes())
                .expect("proof must exist for inserted leaf"),
        })
        .collect();
    StateChunk {
        seq,
        tip_height,
        accounts,
        root,
        compressed: false,
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
fn state_chunk_serialization_orders_accounts() {
    let entries = vec![("carol", 3_000, 5), ("alice", 1_000, 7), ("bob", 2_500, 11)];

    let expected = binary::encode(&build_chunk(5, 10, &entries)).expect("encode baseline chunk");

    for perm in permutations(&entries) {
        let chunk = build_chunk(5, 10, &perm);
        let encoded = binary::encode(&chunk).expect("encode permuted chunk");
        assert_eq!(encoded, expected);
    }
}

#[test]
fn validates_proofs_and_rejects_stale_updates() {
    let dir = tempdir().unwrap();
    let cache_path = dir.path().join("cache.bin");
    let mut stream = StateStream::builder().cache_path(cache_path).build();

    let chunk0 = build_chunk(0, 1, &[("alice", 50, 10)]);
    stream.apply_chunk(chunk0).expect("first chunk");

    let mut invalid_chunk = build_chunk(1, 2, &[("alice", 55, 11)]);
    invalid_chunk.root = [1u8; 32];
    match stream.apply_chunk(invalid_chunk) {
        Err(StateStreamError::InvalidProof { address }) => assert_eq!(address, "alice"),
        other => panic!("expected invalid proof error, got {other:?}"),
    }

    let stale_chunk = build_chunk(1, 2, &[("alice", 60, 9)]);
    match stream.apply_chunk(stale_chunk) {
        Err(StateStreamError::StaleAccountUpdate {
            address,
            cached_seq,
            update_seq,
        }) => {
            assert_eq!(address, "alice");
            assert_eq!(cached_seq, 10);
            assert_eq!(update_seq, 9);
        }
        other => panic!("expected stale update error, got {other:?}"),
    }
}

#[test]
fn gap_recovery_with_callback() {
    let dir = tempdir().unwrap();
    let cache_path = dir.path().join("cache.bin");

    let missing_chunk = build_chunk(0, 1, &[("alice", 5, 1)]);
    let delivered_chunk = build_chunk(1, 2, &[("bob", 7, 3)]);

    let invocations = Arc::new(Mutex::new(0u32));
    let fetch_invocations = invocations.clone();
    let missing_clone = missing_chunk.clone();

    let mut stream = StateStreamBuilder::new()
        .cache_path(cache_path.clone())
        .gap_fetcher(move |from, to| {
            let mut calls = fetch_invocations.lock().unwrap();
            *calls += 1;
            assert_eq!(from, 0);
            assert_eq!(to, 1);
            Ok(vec![missing_clone.clone()])
        })
        .build();

    stream
        .apply_chunk(delivered_chunk)
        .expect("gap should be filled by callback");

    assert_eq!(*invocations.lock().unwrap(), 1);
    assert_eq!(stream.next_seq(), 2);
    assert_eq!(stream.cached_balance("alice"), Some((5, 1)));
    assert_eq!(stream.cached_balance("bob"), Some((7, 3)));
}

#[test]
fn snapshot_resume_persists_state() {
    let dir = tempdir().unwrap();
    let cache_path = dir.path().join("cache.bin");
    let mut stream = StateStream::builder()
        .cache_path(cache_path.clone())
        .build();

    let snapshot = SnapshotPayload {
        accounts: vec![SnapshotAccount {
            address: "alice".to_string(),
            balance: 99,
            seq: 42,
        }],
        next_seq: 5,
    };
    let bytes = binary::encode(&snapshot).unwrap();
    stream
        .apply_snapshot(&bytes, false)
        .expect("snapshot should load");
    assert_eq!(stream.next_seq(), 5);
    assert_eq!(stream.cached_balance("alice"), Some((99, 42)));

    drop(stream);

    let restored = StateStream::builder().cache_path(cache_path).build();
    assert_eq!(restored.next_seq(), 5);
    assert_eq!(restored.cached_balance("alice"), Some((99, 42)));
}

#[test]
fn snapshot_resume_supports_compressed_payloads() {
    let dir = tempdir().unwrap();
    let cache_path = dir.path().join("cache.bin");
    let mut stream = StateStream::builder()
        .cache_path(cache_path.clone())
        .build();

    let snapshot = SnapshotPayload {
        accounts: vec![
            SnapshotAccount {
                address: "eve".to_string(),
                balance: 12,
                seq: 2,
            },
            SnapshotAccount {
                address: "dan".to_string(),
                balance: 9,
                seq: 5,
            },
        ],
        next_seq: 4,
    };

    let encoded = binary::encode(&snapshot).expect("encode snapshot");
    let compressor = coding::compressor_for("lz77-rle", 4).expect("create compressor");
    let compressed = compressor.compress(&encoded).expect("compress snapshot");

    stream
        .apply_snapshot(&compressed, true)
        .expect("compressed snapshot should load");
    assert_eq!(stream.next_seq(), 4);
    assert_eq!(stream.cached_balance("dan"), Some((9, 5)));
    assert_eq!(stream.cached_balance("eve"), Some((12, 2)));

    drop(stream);

    let restored = StateStream::builder().cache_path(cache_path).build();
    assert_eq!(restored.next_seq(), 4);
    assert_eq!(restored.cached_balance("dan"), Some((9, 5)));
    assert_eq!(restored.cached_balance("eve"), Some((12, 2)));
}

#[test]
fn snapshot_randomized_roundtrip_is_deterministic() {
    let mut rng = StdRng::seed_from_u64(0x5eed_beef);
    let mut stream = StateStream::builder().build();

    for iteration in 0..64 {
        let account_count = rng.gen_range(1..=12);
        let mut accounts = Vec::with_capacity(account_count);
        for idx in 0..account_count {
            let suffix = rng.gen::<u32>();
            accounts.push(SnapshotAccount {
                address: format!("acct_{idx}_{suffix:08x}"),
                balance: rng.gen_range(1..=50_000),
                seq: rng.gen_range(0..=128),
            });
        }
        let mut shuffled = accounts.clone();
        shuffled.shuffle(&mut rng);
        let next_seq = rng.gen_range(0..=10_000);
        let payload = SnapshotPayload {
            accounts: shuffled,
            next_seq,
        };

        let encoded = binary::encode(&payload).expect("encode snapshot");
        let mut sorted_accounts = accounts.clone();
        sorted_accounts.sort_by(|a, b| a.address.cmp(&b.address));
        let sorted_payload = SnapshotPayload {
            accounts: sorted_accounts.clone(),
            next_seq,
        };
        let sorted_encoded = binary::encode(&sorted_payload).expect("encode sorted snapshot");
        assert_eq!(
            encoded, sorted_encoded,
            "bytes differ on iteration {}",
            iteration
        );

        let decoded: SnapshotPayload = binary::decode(&encoded).expect("decode snapshot");
        assert_eq!(decoded.accounts, sorted_accounts, "decoded order mismatch");

        let use_compression = rng.gen_bool(0.5);
        let (payload_bytes, compressed) = if use_compression {
            let compressor = coding::compressor_for("lz77-rle", 4).expect("compressor");
            (
                compressor.compress(&encoded).expect("compress snapshot"),
                true,
            )
        } else {
            (encoded.clone(), false)
        };

        stream
            .apply_snapshot(&payload_bytes, compressed)
            .expect("apply snapshot");
        assert_eq!(stream.next_seq(), next_seq);
        for account in &accounts {
            assert_eq!(
                stream.cached_balance(&account.address),
                Some((account.balance, account.seq)),
                "cache mismatch for {} on iteration {iteration}",
                account.address
            );
        }
    }
}

#[test]
fn legacy_hashmap_snapshot_roundtrip_randomized() {
    let mut rng = StdRng::seed_from_u64(0x51de_cade);
    let mut stream = StateStream::builder().build();

    for iteration in 0..48 {
        let entry_count = rng.gen_range(1..=10);
        let mut legacy = HashMap::with_capacity(entry_count);
        for idx in 0..entry_count {
            let balance = rng.gen_range(1..=75_000);
            let address = format!("legacy_{idx}_{:08x}", rng.gen::<u32>());
            legacy.insert(address, balance);
        }

        let encoded = binary::encode(&legacy).expect("encode legacy map");
        stream
            .apply_snapshot(&encoded, false)
            .expect("apply legacy snapshot");
        assert_eq!(
            stream.next_seq(),
            0,
            "legacy snapshot should reset next_seq"
        );
        for (address, balance) in &legacy {
            assert_eq!(
                stream.cached_balance(address),
                Some((*balance, 0)),
                "legacy cache mismatch for {} iteration {}",
                address,
                iteration
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn chunk_persist_failure_rolls_back_state() {
    use std::fs;

    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("ro");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("cache.bin");

    let mut stream = StateStream::builder()
        .cache_path(cache_path.clone())
        .build();
    let chunk = build_chunk(0, 0, &[("alice", 1, 1)]);

    fs::create_dir_all(&cache_path).unwrap();

    let err = stream
        .apply_chunk(chunk)
        .expect_err("persist failure should bubble up");
    assert!(matches!(err, StateStreamError::Io(_)));
    assert_eq!(stream.next_seq(), 0);
    assert_eq!(stream.cached_balance("alice"), None);
    fs::remove_dir_all(&cache_path).unwrap();
}

#[cfg(unix)]
#[test]
fn snapshot_persist_failure_rolls_back_state() {
    use std::fs;

    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("persist");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("cache.bin");

    let mut stream = StateStream::builder()
        .cache_path(cache_path.clone())
        .build();
    let chunk = build_chunk(0, 1, &[("carol", 4, 2)]);
    stream
        .apply_chunk(chunk)
        .expect("initial chunk should persist");

    fs::remove_file(&cache_path).unwrap();
    fs::create_dir_all(&cache_path).unwrap();

    let snapshot = SnapshotPayload {
        accounts: vec![SnapshotAccount {
            address: "dave".to_string(),
            balance: 99,
            seq: 10,
        }],
        next_seq: 4,
    };
    let bytes = binary::encode(&snapshot).unwrap();
    let err = stream
        .apply_snapshot(&bytes, false)
        .expect_err("snapshot persist failure should bubble up");
    assert!(matches!(err, StateStreamError::Io(_)));
    assert_eq!(stream.next_seq(), 1);
    assert_eq!(stream.cached_balance("carol"), Some((4, 2)));
    assert_eq!(stream.cached_balance("dave"), None);
    fs::remove_dir_all(&cache_path).unwrap();
}
