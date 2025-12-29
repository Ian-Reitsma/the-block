use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;

use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::accounts::abstraction::SessionPolicy;
use crate::block_binary;
use crate::blockchain::macro_block::MacroBlock;
use crate::economics;
use crate::localnet::AssistReceipt;
use crate::transaction::binary::{self as tx_binary, EncodeError, EncodeResult};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};
use crate::{Account, Block, ChainDisk, MempoolEntryDisk, Params, TokenAmount, TokenBalance};

/// Encode an [`Account`] into the canonical binary layout.
pub fn encode_account(account: &Account) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(256);
    write_account(&mut writer, account)?;
    Ok(writer.finish())
}

/// Decode an [`Account`] previously produced by [`encode_account`].
pub fn decode_account(bytes: &[u8]) -> binary_struct::Result<Account> {
    let mut reader = Reader::new(bytes);
    let account = read_account(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(account)
}

/// Encode a [`ChainDisk`] snapshot using the in-house binary layout.
pub fn encode_chain_disk(disk: &ChainDisk) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(4096);
    write_chain_disk(&mut writer, disk)?;
    Ok(writer.finish())
}

/// Decode a [`ChainDisk`] snapshot produced by [`encode_chain_disk`].
pub fn decode_chain_disk(bytes: &[u8]) -> binary_struct::Result<ChainDisk> {
    let mut reader = Reader::new(bytes);
    let disk = read_chain_disk(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(disk)
}

/// Decode a vector of [`Block`] values using the canonical cursor layout.
pub fn decode_block_vec(bytes: &[u8]) -> binary_struct::Result<Vec<Block>> {
    let mut reader = Reader::new(bytes);
    let blocks = read_vec(&mut reader, "blocks", block_binary::read_block)?;
    ensure_exhausted(&reader)?;
    Ok(blocks)
}

/// Decode a legacy account map using the canonical cursor layout.
pub fn decode_account_map_bytes(bytes: &[u8]) -> binary_struct::Result<HashMap<String, Account>> {
    let mut reader = Reader::new(bytes);
    let accounts = read_account_map(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(accounts)
}

/// Decode the legacy emission tuple `(em_c, em_i, br_c, br_i, block_height)`.
pub fn decode_emission_tuple(bytes: &[u8]) -> binary_struct::Result<(u64, u64, u64, u64, u64)> {
    let mut reader = Reader::new(bytes);
    let mut emission_consumer = None;
    let mut emission_industrial = None;
    let mut block_reward_consumer = None;
    let mut block_reward_industrial = None;
    let mut block_height = None;

    decode_struct(&mut reader, Some(5), |key, reader| match key {
        "emission_consumer" => assign_once(
            &mut emission_consumer,
            reader.read_u64()?,
            "emission_consumer",
        ),
        "emission_industrial" => assign_once(
            &mut emission_industrial,
            reader.read_u64()?,
            "emission_industrial",
        ),
        "block_reward_consumer" => assign_once(
            &mut block_reward_consumer,
            reader.read_u64()?,
            "block_reward_consumer",
        ),
        "block_reward_industrial" => assign_once(
            &mut block_reward_industrial,
            reader.read_u64()?,
            "block_reward_industrial",
        ),
        "block_height" => assign_once(&mut block_height, reader.read_u64()?, "block_height"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    ensure_exhausted(&reader)?;

    Ok((
        emission_consumer.unwrap_or_default(),
        emission_industrial.unwrap_or_default(),
        block_reward_consumer.unwrap_or_default(),
        block_reward_industrial.unwrap_or_default(),
        block_height.unwrap_or_default(),
    ))
}

/// Encode a [`MacroBlock`] checkpoint.
pub fn encode_macro_block(block: &MacroBlock) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(256);
    write_macro_block(&mut writer, block)?;
    Ok(writer.finish())
}

/// Decode a [`MacroBlock`] checkpoint.
pub fn decode_macro_block(bytes: &[u8]) -> binary_struct::Result<MacroBlock> {
    let mut reader = Reader::new(bytes);
    let block = read_macro_block(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(block)
}

/// Encode an [`AssistReceipt`] for hashing and persistence.
pub fn encode_assist_receipt(receipt: &AssistReceipt) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(128);
    write_assist_receipt(&mut writer, receipt)?;
    Ok(writer.finish())
}

/// Encode a schema version marker as canonical binary.
pub fn encode_schema_version(version: u32) -> Vec<u8> {
    let mut writer = Writer::with_capacity(16);
    writer.write_u64(1);
    writer.write_string("version");
    writer.write_u32(version);
    writer.finish()
}

/// Decode a schema version marker previously written by [`encode_schema_version`].
pub fn decode_schema_version(bytes: &[u8]) -> Option<u32> {
    let mut reader = Reader::new(bytes);
    let mut version = None;
    let result = decode_struct(&mut reader, Some(1), |key, reader| match key {
        "version" => {
            version = Some(reader.read_u32()?);
            Ok(())
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    });
    if result.is_ok() && ensure_exhausted(&reader).is_ok() {
        version
    } else {
        None
    }
}

fn write_account(writer: &mut Writer, account: &Account) -> EncodeResult<()> {
    writer.write_u64(7);
    writer.write_string("address");
    writer.write_string(&account.address);
    writer.write_string("balance");
    write_balance(writer, &account.balance)?;
    writer.write_string("nonce");
    writer.write_u64(account.nonce);
    writer.write_string("pending_amount");
    writer.write_u64(account.pending_amount);
    writer.write_string("pending_nonce");
    writer.write_u64(account.pending_nonce);
    writer.write_string("pending_nonces");
    let mut pending: Vec<u64> = account.pending_nonces.iter().copied().collect();
    pending.sort_unstable();
    write_u64_vec(writer, &pending, "pending_nonces")?;
    writer.write_string("sessions");
    write_vec(writer, &account.sessions, "sessions", write_session)?;
    Ok(())
}

fn read_account(reader: &mut Reader<'_>) -> binary_struct::Result<Account> {
    let mut address = None;
    let mut balance = None;
    let mut nonce = None;
    let mut pending_amount = None;
    let mut pending_consumer_legacy = None;
    let mut pending_industrial_legacy = None;
    let mut pending_nonce = None;
    let mut pending_nonces = None;
    let mut sessions = None;

    decode_struct(reader, None, |key, reader| match key {
        "address" => assign_once(&mut address, reader.read_string()?, "address"),
        "balance" => assign_once(&mut balance, read_balance(reader)?, "balance"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "pending_amount" => assign_once(&mut pending_amount, reader.read_u64()?, "pending_amount"),
        // Legacy fields for backward compatibility
        "pending_consumer" => assign_once(
            &mut pending_consumer_legacy,
            reader.read_u64()?,
            "pending_consumer",
        ),
        "pending_industrial" => assign_once(
            &mut pending_industrial_legacy,
            reader.read_u64()?,
            "pending_industrial",
        ),
        "pending_nonce" => assign_once(&mut pending_nonce, reader.read_u64()?, "pending_nonce"),
        "pending_nonces" => assign_once(
            &mut pending_nonces,
            read_u64_vec(reader, "pending_nonces")?,
            "pending_nonces",
        ),
        "sessions" => assign_once(
            &mut sessions,
            read_vec(reader, "sessions", read_session)?,
            "sessions",
        ),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    // Compute pending_amount from legacy fields if new field not present
    let final_pending_amount = pending_amount.or_else(|| {
        match (pending_consumer_legacy, pending_industrial_legacy) {
            (Some(c), Some(i)) => Some(c + i),
            _ => None,
        }
    }).unwrap_or_default();

    Ok(Account {
        address: address.ok_or(DecodeError::MissingField("address"))?,
        balance: balance.unwrap_or(TokenBalance {
            amount: 0,
        }),
        nonce: nonce.unwrap_or_default(),
        pending_amount: final_pending_amount,
        pending_nonce: pending_nonce.unwrap_or_default(),
        pending_nonces: pending_nonces.unwrap_or_default().into_iter().collect(),
        sessions: sessions.unwrap_or_default(),
    })
}

fn write_balance(writer: &mut Writer, balance: &TokenBalance) -> EncodeResult<()> {
    writer.write_u64(1);
    writer.write_string("amount");
    writer.write_u64(balance.amount);
    Ok(())
}

fn read_balance(reader: &mut Reader<'_>) -> binary_struct::Result<TokenBalance> {
    let mut amount = None;
    let mut consumer_legacy = None;
    let mut industrial_legacy = None;

    // Accept 1 field (new format) or 2 fields (legacy format) for backward compatibility
    decode_struct(reader, None, |key, reader| match key {
        "amount" => assign_once(&mut amount, reader.read_u64()?, "amount"),
        // Legacy fields for backward compatibility
        "consumer" => assign_once(&mut consumer_legacy, reader.read_u64()?, "consumer"),
        "industrial" => assign_once(&mut industrial_legacy, reader.read_u64()?, "industrial"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    // If reading legacy format, sum consumer + industrial
    let final_amount = amount.or_else(|| {
        match (consumer_legacy, industrial_legacy) {
            (Some(c), Some(i)) => Some(c + i),
            _ => None,
        }
    }).unwrap_or_default();

    Ok(TokenBalance {
        amount: final_amount,
    })
}

fn write_session(writer: &mut Writer, session: &SessionPolicy) -> EncodeResult<()> {
    writer.write_u64(3);
    writer.write_string("public_key");
    write_bytes(writer, &session.public_key, "public_key")?;
    writer.write_string("expires_at");
    writer.write_u64(session.expires_at);
    writer.write_string("nonce");
    writer.write_u64(session.nonce);
    Ok(())
}

fn read_session(reader: &mut Reader<'_>) -> binary_struct::Result<SessionPolicy> {
    let mut public_key = None;
    let mut expires_at = None;
    let mut nonce = None;

    decode_struct(reader, Some(3), |key, reader| match key {
        "public_key" => assign_once(&mut public_key, reader.read_bytes()?, "public_key"),
        "expires_at" => assign_once(&mut expires_at, reader.read_u64()?, "expires_at"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(SessionPolicy {
        public_key: public_key.unwrap_or_default(),
        expires_at: expires_at.unwrap_or_default(),
        nonce: nonce.unwrap_or_default(),
    })
}

fn write_chain_disk(writer: &mut Writer, disk: &ChainDisk) -> EncodeResult<()> {
    writer.write_u64(30); // Updated to match the current number of encoded fields
    writer.write_string("schema_version");
    writer.write_u64(disk.schema_version as u64);
    writer.write_string("chain");
    write_vec(writer, &disk.chain, "chain", block_binary::write_block)?;
    writer.write_string("accounts");
    write_account_map(writer, &disk.accounts)?;
    writer.write_string("emission");
    writer.write_u64(disk.emission);
    writer.write_string("emission_year_ago");
    writer.write_u64(disk.emission_year_ago);
    writer.write_string("inflation_epoch_marker");
    writer.write_u64(disk.inflation_epoch_marker);
    writer.write_string("block_reward");
    writer.write_u64(disk.block_reward.get());
    writer.write_string("block_height");
    writer.write_u64(disk.block_height);
    writer.write_string("mempool");
    write_vec(writer, &disk.mempool, "mempool", write_mempool_entry)?;
    writer.write_string("base_fee");
    writer.write_u64(disk.base_fee);
    writer.write_string("params");
    write_params(writer, &disk.params)?;
    writer.write_string("epoch_storage_bytes");
    writer.write_u64(disk.epoch_storage_bytes);
    writer.write_string("epoch_read_bytes");
    writer.write_u64(disk.epoch_read_bytes);
    writer.write_string("epoch_cpu_ms");
    writer.write_u64(disk.epoch_cpu_ms);
    writer.write_string("epoch_bytes_out");
    writer.write_u64(disk.epoch_bytes_out);
    writer.write_string("recent_timestamps");
    write_u64_vec(writer, &disk.recent_timestamps, "recent_timestamps")?;
    writer.write_string("economics_block_reward_per_block");
    writer.write_u64(disk.economics_block_reward_per_block);
    writer.write_string("economics_prev_annual_issuance_block");
    writer.write_u64(disk.economics_prev_annual_issuance_block);
    writer.write_string("economics_prev_subsidy");
    write_subsidy_snapshot(writer, &disk.economics_prev_subsidy)?;
    writer.write_string("economics_prev_tariff");
    write_tariff_snapshot(writer, &disk.economics_prev_tariff)?;
    writer.write_string("economics_prev_market_metrics");
    write_market_metrics(writer, &disk.economics_prev_market_metrics)?;
    writer.write_string("economics_epoch_tx_volume_block");
    writer.write_u64(disk.economics_epoch_tx_volume_block);
    writer.write_string("economics_epoch_tx_count");
    writer.write_u64(disk.economics_epoch_tx_count);
    writer.write_string("economics_epoch_treasury_inflow_block");
    writer.write_u64(disk.economics_epoch_treasury_inflow_block);
    writer.write_string("economics_epoch_storage_payout_block");
    writer.write_u64(disk.economics_epoch_storage_payout_block);
    writer.write_string("economics_epoch_compute_payout_block");
    writer.write_u64(disk.economics_epoch_compute_payout_block);
    writer.write_string("economics_epoch_ad_payout_block");
    writer.write_u64(disk.economics_epoch_ad_payout_block);
    writer.write_string("economics_baseline_tx_count");
    writer.write_u64(disk.economics_baseline_tx_count);
    writer.write_string("economics_baseline_tx_volume");
    writer.write_u64(disk.economics_baseline_tx_volume);
    writer.write_string("economics_baseline_miners");
    writer.write_u64(disk.economics_baseline_miners);
    Ok(())
}

fn read_chain_disk(reader: &mut Reader<'_>) -> binary_struct::Result<ChainDisk> {
    let mut schema_version = None;
    let mut chain = None;
    let mut accounts = None;
    let mut emission = None;
    let mut emission_consumer = None;
    let mut emission_industrial = None;
    let mut emission_year_ago = None;
    let mut emission_consumer_year_ago = None;
    let mut inflation_epoch_marker = None;
    let mut block_reward = None;
    let mut block_reward_consumer = None;
    let mut block_reward_industrial = None;
    let mut block_height = None;
    let mut mempool = None;
    let mut base_fee = None;
    let mut params = None;
    let mut epoch_storage_bytes = None;
    let mut epoch_read_bytes = None;
    let mut epoch_cpu_ms = None;
    let mut epoch_bytes_out = None;
    let mut recent_timestamps = None;
    let mut economics_block_reward_per_block = None;
    let mut economics_prev_annual_issuance_block = None;
    let mut economics_prev_subsidy = None;
    let mut economics_prev_tariff = None;
    let mut economics_prev_market_metrics = None;
    let mut economics_epoch_tx_volume_block = None;
    let mut economics_epoch_tx_count = None;
    let mut economics_epoch_treasury_inflow_block = None;
    let mut economics_epoch_storage_payout_block = None;
    let mut economics_epoch_compute_payout_block = None;
    let mut economics_epoch_ad_payout_block = None;
    let mut economics_baseline_tx_count = None;
    let mut economics_baseline_tx_volume = None;
    let mut economics_baseline_miners = None;

    decode_struct(reader, None, |key, reader| match key {
        // Changed from Some(18) to support both old (18) and new (16) field formats
        "schema_version" => assign_once(
            &mut schema_version,
            reader.read_u64().map(|v| v as usize)?,
            "schema_version",
        ),
        "chain" => assign_once(
            &mut chain,
            read_vec(reader, "chain", block_binary::read_block)?,
            "chain",
        ),
        "accounts" => assign_once(&mut accounts, read_account_map(reader)?, "accounts"),
        "emission" => assign_once(&mut emission, reader.read_u64()?, "emission"),
        "emission_consumer" => assign_once(
            &mut emission_consumer,
            reader.read_u64()?,
            "emission_consumer",
        ),
        "emission_industrial" => assign_once(
            &mut emission_industrial,
            reader.read_u64()?,
            "emission_industrial",
        ),
        "emission_year_ago" => assign_once(
            &mut emission_year_ago,
            reader.read_u64()?,
            "emission_year_ago",
        ),
        "emission_consumer_year_ago" => assign_once(
            &mut emission_consumer_year_ago,
            reader.read_u64()?,
            "emission_consumer_year_ago",
        ),
        "inflation_epoch_marker" => assign_once(
            &mut inflation_epoch_marker,
            reader.read_u64()?,
            "inflation_epoch_marker",
        ),
        "block_reward" => assign_once(&mut block_reward, reader.read_u64()?, "block_reward"),
        "block_reward_consumer" => assign_once(
            &mut block_reward_consumer,
            reader.read_u64()?,
            "block_reward_consumer",
        ),
        "block_reward_industrial" => assign_once(
            &mut block_reward_industrial,
            reader.read_u64()?,
            "block_reward_industrial",
        ),
        "block_height" => assign_once(&mut block_height, reader.read_u64()?, "block_height"),
        "mempool" => assign_once(
            &mut mempool,
            read_vec(reader, "mempool", read_mempool_entry)?,
            "mempool",
        ),
        "base_fee" => assign_once(&mut base_fee, reader.read_u64()?, "base_fee"),
        "params" => assign_once(&mut params, read_params(reader)?, "params"),
        "epoch_storage_bytes" => assign_once(
            &mut epoch_storage_bytes,
            reader.read_u64()?,
            "epoch_storage_bytes",
        ),
        "epoch_read_bytes" => assign_once(
            &mut epoch_read_bytes,
            reader.read_u64()?,
            "epoch_read_bytes",
        ),
        "epoch_cpu_ms" => assign_once(&mut epoch_cpu_ms, reader.read_u64()?, "epoch_cpu_ms"),
        "epoch_bytes_out" => {
            assign_once(&mut epoch_bytes_out, reader.read_u64()?, "epoch_bytes_out")
        }
        "recent_timestamps" => assign_once(
            &mut recent_timestamps,
            read_u64_vec(reader, "recent_timestamps")?,
            "recent_timestamps",
        ),
        "economics_block_reward_per_block" => assign_once(
            &mut economics_block_reward_per_block,
            reader.read_u64()?,
            "economics_block_reward_per_block",
        ),
        "economics_prev_annual_issuance_block" => assign_once(
            &mut economics_prev_annual_issuance_block,
            reader.read_u64()?,
            "economics_prev_annual_issuance_block",
        ),
        "economics_prev_subsidy" => assign_once(
            &mut economics_prev_subsidy,
            read_subsidy_snapshot(reader)?,
            "economics_prev_subsidy",
        ),
        "economics_prev_tariff" => assign_once(
            &mut economics_prev_tariff,
            read_tariff_snapshot(reader)?,
            "economics_prev_tariff",
        ),
        "economics_prev_market_metrics" => assign_once(
            &mut economics_prev_market_metrics,
            read_market_metrics(reader)?,
            "economics_prev_market_metrics",
        ),
        "economics_epoch_tx_volume_block" => assign_once(
            &mut economics_epoch_tx_volume_block,
            reader.read_u64()?,
            "economics_epoch_tx_volume_block",
        ),
        "economics_epoch_tx_count" => assign_once(
            &mut economics_epoch_tx_count,
            reader.read_u64()?,
            "economics_epoch_tx_count",
        ),
        "economics_epoch_treasury_inflow_block" => assign_once(
            &mut economics_epoch_treasury_inflow_block,
            reader.read_u64()?,
            "economics_epoch_treasury_inflow_block",
        ),
        "economics_epoch_storage_payout_block" => assign_once(
            &mut economics_epoch_storage_payout_block,
            reader.read_u64()?,
            "economics_epoch_storage_payout_block",
        ),
        "economics_epoch_compute_payout_block" => assign_once(
            &mut economics_epoch_compute_payout_block,
            reader.read_u64()?,
            "economics_epoch_compute_payout_block",
        ),
        "economics_epoch_ad_payout_block" => assign_once(
            &mut economics_epoch_ad_payout_block,
            reader.read_u64()?,
            "economics_epoch_ad_payout_block",
        ),
        "economics_baseline_tx_count" => assign_once(
            &mut economics_baseline_tx_count,
            reader.read_u64()?,
            "economics_baseline_tx_count",
        ),
        "economics_baseline_tx_volume" => assign_once(
            &mut economics_baseline_tx_volume,
            reader.read_u64()?,
            "economics_baseline_tx_volume",
        ),
        "economics_baseline_miners" => assign_once(
            &mut economics_baseline_miners,
            reader.read_u64()?,
            "economics_baseline_miners",
        ),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(ChainDisk {
        schema_version: schema_version.unwrap_or_default(),
        chain: chain.unwrap_or_default(),
        accounts: accounts.unwrap_or_default(),
        emission: emission.unwrap_or_else(|| {
            emission_consumer.unwrap_or_default() + emission_industrial.unwrap_or_default()
        }),
        emission_year_ago: emission_year_ago
            .unwrap_or_else(|| emission_consumer_year_ago.unwrap_or_default()),
        inflation_epoch_marker: inflation_epoch_marker.unwrap_or_default(),
        block_reward: TokenAmount::new(block_reward.unwrap_or_else(|| {
            block_reward_consumer.unwrap_or_default() + block_reward_industrial.unwrap_or_default()
        })),
        block_height: block_height.unwrap_or_default(),
        mempool: mempool.unwrap_or_default(),
        base_fee: base_fee.unwrap_or_default(),
        params: params.unwrap_or_default(),
        epoch_storage_bytes: epoch_storage_bytes.unwrap_or_default(),
        epoch_read_bytes: epoch_read_bytes.unwrap_or_default(),
        epoch_cpu_ms: epoch_cpu_ms.unwrap_or_default(),
        epoch_bytes_out: epoch_bytes_out.unwrap_or_default(),
        recent_timestamps: recent_timestamps.unwrap_or_default(),
        economics_block_reward_per_block: economics_block_reward_per_block.unwrap_or_default(),
        economics_prev_annual_issuance_block: economics_prev_annual_issuance_block
            .unwrap_or_default(),
        economics_prev_subsidy: economics_prev_subsidy.unwrap_or_default(),
        economics_prev_tariff: economics_prev_tariff.unwrap_or_default(),
        economics_prev_market_metrics: economics_prev_market_metrics.unwrap_or_default(),
        economics_epoch_tx_volume_block: economics_epoch_tx_volume_block.unwrap_or_default(),
        economics_epoch_tx_count: economics_epoch_tx_count.unwrap_or_default(),
        economics_epoch_treasury_inflow_block: economics_epoch_treasury_inflow_block
            .unwrap_or_default(),
        economics_epoch_storage_payout_block: economics_epoch_storage_payout_block
            .unwrap_or_default(),
        economics_epoch_compute_payout_block: economics_epoch_compute_payout_block
            .unwrap_or_default(),
        economics_epoch_ad_payout_block: economics_epoch_ad_payout_block.unwrap_or_default(),
        economics_baseline_tx_count: economics_baseline_tx_count.unwrap_or(100),
        economics_baseline_tx_volume: economics_baseline_tx_volume.unwrap_or(10_000),
        economics_baseline_miners: economics_baseline_miners.unwrap_or(10),
    })
}

fn write_account_map(writer: &mut Writer, accounts: &HashMap<String, Account>) -> EncodeResult<()> {
    let mut entries: BTreeMap<&String, &Account> = BTreeMap::new();
    for (k, v) in accounts {
        entries.insert(k, v);
    }
    let len = u64::try_from(entries.len()).map_err(|_| EncodeError::LengthOverflow("accounts"))?;
    writer.write_u64(len);
    for (key, value) in entries {
        writer.write_string(key);
        write_account(writer, value)?;
    }
    Ok(())
}

fn read_account_map(reader: &mut Reader<'_>) -> binary_struct::Result<HashMap<String, Account>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut map = HashMap::with_capacity(len);
    for _ in 0..len {
        let key = reader.read_string()?;
        let value = read_account(reader)?;
        map.insert(key, value);
    }
    Ok(map)
}

fn write_mempool_entry(writer: &mut Writer, entry: &MempoolEntryDisk) -> EncodeResult<()> {
    writer.write_u64(6);
    writer.write_string("sender");
    writer.write_string(&entry.sender);
    writer.write_string("nonce");
    writer.write_u64(entry.nonce);
    writer.write_string("tx");
    tx_binary::write_signed_transaction(writer, &entry.tx)?;
    writer.write_string("timestamp_millis");
    writer.write_u64(entry.timestamp_millis);
    writer.write_string("timestamp_ticks");
    writer.write_u64(entry.timestamp_ticks);
    writer.write_string("serialized_size");
    writer.write_u64(entry.serialized_size);
    Ok(())
}

fn read_mempool_entry(reader: &mut Reader<'_>) -> binary_struct::Result<MempoolEntryDisk> {
    let mut sender = None;
    let mut nonce = None;
    let mut tx = None;
    let mut timestamp_millis = None;
    let mut timestamp_ticks = None;
    let mut serialized_size = None;

    decode_struct(reader, None, |key, reader| match key {
        "sender" => assign_once(&mut sender, reader.read_string()?, "sender"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "tx" => assign_once(&mut tx, tx_binary::read_signed_transaction(reader)?, "tx"),
        "timestamp_millis" => assign_once(
            &mut timestamp_millis,
            reader.read_u64()?,
            "timestamp_millis",
        ),
        "timestamp_ticks" => {
            assign_once(&mut timestamp_ticks, reader.read_u64()?, "timestamp_ticks")
        }
        "serialized_size" => {
            assign_once(&mut serialized_size, reader.read_u64()?, "serialized_size")
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(MempoolEntryDisk {
        sender: sender.unwrap_or_default(),
        nonce: nonce.unwrap_or_default(),
        tx: tx.ok_or(DecodeError::MissingField("tx"))?,
        timestamp_millis: timestamp_millis.unwrap_or_default(),
        timestamp_ticks: timestamp_ticks.unwrap_or_default(),
        serialized_size: serialized_size.unwrap_or_default(),
    })
}

fn write_params(writer: &mut Writer, params: &Params) -> EncodeResult<()> {
    writer.write_u64(39);
    writer.write_string("snapshot_interval_secs");
    writer.write_i64(params.snapshot_interval_secs);
    writer.write_string("consumer_fee_comfort_p90_microunits");
    writer.write_i64(params.consumer_fee_comfort_p90_microunits);
    writer.write_string("fee_floor_window");
    writer.write_i64(params.fee_floor_window);
    writer.write_string("fee_floor_percentile");
    writer.write_i64(params.fee_floor_percentile);
    writer.write_string("industrial_admission_min_capacity");
    writer.write_i64(params.industrial_admission_min_capacity);
    writer.write_string("fairshare_global_max_ppm");
    writer.write_i64(params.fairshare_global_max_ppm);
    writer.write_string("burst_refill_rate_per_s_ppm");
    writer.write_i64(params.burst_refill_rate_per_s_ppm);
    writer.write_string("beta_storage_sub");
    writer.write_i64(params.beta_storage_sub);
    writer.write_string("gamma_read_sub");
    writer.write_i64(params.gamma_read_sub);
    writer.write_string("kappa_cpu_sub");
    writer.write_i64(params.kappa_cpu_sub);
    writer.write_string("lambda_bytes_out_sub");
    writer.write_i64(params.lambda_bytes_out_sub);
    writer.write_string("treasury_percent");
    writer.write_i64(params.treasury_percent);
    writer.write_string("proof_rebate_limit");
    writer.write_i64(params.proof_rebate_limit);
    writer.write_string("rent_rate_per_byte");
    writer.write_i64(params.rent_rate_per_byte);
    writer.write_string("kill_switch_subsidy_reduction");
    writer.write_i64(params.kill_switch_subsidy_reduction);
    writer.write_string("miner_reward_logistic_target");
    writer.write_i64(params.miner_reward_logistic_target);
    writer.write_string("logistic_slope_milli");
    writer.write_i64(params.logistic_slope_milli);
    writer.write_string("miner_hysteresis");
    writer.write_i64(params.miner_hysteresis);
    writer.write_string("risk_lambda");
    writer.write_i64(params.risk_lambda);
    writer.write_string("entropy_phi");
    writer.write_i64(params.entropy_phi);
    writer.write_string("haar_eta");
    writer.write_i64(params.haar_eta);
    writer.write_string("util_var_threshold");
    writer.write_i64(params.util_var_threshold);
    writer.write_string("fib_window_base_secs");
    writer.write_i64(params.fib_window_base_secs);
    writer.write_string("heuristic_mu_milli");
    writer.write_i64(params.heuristic_mu_milli);
    writer.write_string("industrial_multiplier");
    writer.write_i64(params.industrial_multiplier);
    writer.write_string("badge_expiry_secs");
    writer.write_i64(params.badge_expiry_secs);
    writer.write_string("badge_issue_uptime_percent");
    writer.write_i64(params.badge_issue_uptime_percent);
    writer.write_string("badge_revoke_uptime_percent");
    writer.write_i64(params.badge_revoke_uptime_percent);
    writer.write_string("jurisdiction_region");
    writer.write_i64(params.jurisdiction_region);
    writer.write_string("ai_diagnostics_enabled");
    writer.write_i64(params.ai_diagnostics_enabled);
    writer.write_string("kalman_r_short");
    writer.write_i64(params.kalman_r_short);
    writer.write_string("kalman_r_med");
    writer.write_i64(params.kalman_r_med);
    writer.write_string("kalman_r_long");
    writer.write_i64(params.kalman_r_long);
    writer.write_string("scheduler_weight_gossip");
    writer.write_i64(params.scheduler_weight_gossip);
    writer.write_string("scheduler_weight_compute");
    writer.write_i64(params.scheduler_weight_compute);
    writer.write_string("scheduler_weight_storage");
    writer.write_i64(params.scheduler_weight_storage);
    writer.write_string("runtime_backend_policy");
    writer.write_i64(params.runtime_backend_policy);
    writer.write_string("transport_provider_policy");
    writer.write_i64(params.transport_provider_policy);
    writer.write_string("storage_engine_policy");
    writer.write_i64(params.storage_engine_policy);
    Ok(())
}

fn read_params(reader: &mut Reader<'_>) -> binary_struct::Result<Params> {
    let mut params = Params::default();

    decode_struct(reader, Some(39), |key, reader| match key {
        "snapshot_interval_secs" => {
            params.snapshot_interval_secs = reader.read_i64()?;
            Ok(())
        }
        "consumer_fee_comfort_p90_microunits" => {
            params.consumer_fee_comfort_p90_microunits = reader.read_i64()?;
            Ok(())
        }
        "fee_floor_window" => {
            params.fee_floor_window = reader.read_i64()?;
            Ok(())
        }
        "fee_floor_percentile" => {
            params.fee_floor_percentile = reader.read_i64()?;
            Ok(())
        }
        "industrial_admission_min_capacity" => {
            params.industrial_admission_min_capacity = reader.read_i64()?;
            Ok(())
        }
        "fairshare_global_max_ppm" => {
            params.fairshare_global_max_ppm = reader.read_i64()?;
            Ok(())
        }
        "burst_refill_rate_per_s_ppm" => {
            params.burst_refill_rate_per_s_ppm = reader.read_i64()?;
            Ok(())
        }
        "beta_storage_sub" => {
            params.beta_storage_sub = reader.read_i64()?;
            Ok(())
        }
        "gamma_read_sub" => {
            params.gamma_read_sub = reader.read_i64()?;
            Ok(())
        }
        "kappa_cpu_sub" => {
            params.kappa_cpu_sub = reader.read_i64()?;
            Ok(())
        }
        "lambda_bytes_out_sub" => {
            params.lambda_bytes_out_sub = reader.read_i64()?;
            Ok(())
        }
        "treasury_percent" => {
            params.treasury_percent = reader.read_i64()?;
            Ok(())
        }
        "proof_rebate_limit" => {
            params.proof_rebate_limit = reader.read_i64()?;
            Ok(())
        }
        "rent_rate_per_byte" => {
            params.rent_rate_per_byte = reader.read_i64()?;
            Ok(())
        }
        "kill_switch_subsidy_reduction" => {
            params.kill_switch_subsidy_reduction = reader.read_i64()?;
            Ok(())
        }
        "miner_reward_logistic_target" => {
            params.miner_reward_logistic_target = reader.read_i64()?;
            Ok(())
        }
        "logistic_slope_milli" => {
            params.logistic_slope_milli = reader.read_i64()?;
            Ok(())
        }
        "miner_hysteresis" => {
            params.miner_hysteresis = reader.read_i64()?;
            Ok(())
        }
        "risk_lambda" => {
            params.risk_lambda = reader.read_i64()?;
            Ok(())
        }
        "entropy_phi" => {
            params.entropy_phi = reader.read_i64()?;
            Ok(())
        }
        "haar_eta" => {
            params.haar_eta = reader.read_i64()?;
            Ok(())
        }
        "util_var_threshold" => {
            params.util_var_threshold = reader.read_i64()?;
            Ok(())
        }
        "fib_window_base_secs" => {
            params.fib_window_base_secs = reader.read_i64()?;
            Ok(())
        }
        "heuristic_mu_milli" => {
            params.heuristic_mu_milli = reader.read_i64()?;
            Ok(())
        }
        "industrial_multiplier" => {
            params.industrial_multiplier = reader.read_i64()?;
            Ok(())
        }
        "badge_expiry_secs" => {
            params.badge_expiry_secs = reader.read_i64()?;
            Ok(())
        }
        "badge_issue_uptime_percent" => {
            params.badge_issue_uptime_percent = reader.read_i64()?;
            Ok(())
        }
        "badge_revoke_uptime_percent" => {
            params.badge_revoke_uptime_percent = reader.read_i64()?;
            Ok(())
        }
        "jurisdiction_region" => {
            params.jurisdiction_region = reader.read_i64()?;
            Ok(())
        }
        "ai_diagnostics_enabled" => {
            params.ai_diagnostics_enabled = reader.read_i64()?;
            Ok(())
        }
        "kalman_r_short" => {
            params.kalman_r_short = reader.read_i64()?;
            Ok(())
        }
        "kalman_r_med" => {
            params.kalman_r_med = reader.read_i64()?;
            Ok(())
        }
        "kalman_r_long" => {
            params.kalman_r_long = reader.read_i64()?;
            Ok(())
        }
        "scheduler_weight_gossip" => {
            params.scheduler_weight_gossip = reader.read_i64()?;
            Ok(())
        }
        "scheduler_weight_compute" => {
            params.scheduler_weight_compute = reader.read_i64()?;
            Ok(())
        }
        "scheduler_weight_storage" => {
            params.scheduler_weight_storage = reader.read_i64()?;
            Ok(())
        }
        "runtime_backend_policy" => {
            params.runtime_backend_policy = reader.read_i64()?;
            Ok(())
        }
        "transport_provider_policy" => {
            params.transport_provider_policy = reader.read_i64()?;
            Ok(())
        }
        "storage_engine_policy" => {
            params.storage_engine_policy = reader.read_i64()?;
            Ok(())
        }
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(params)
}

fn write_subsidy_snapshot(
    writer: &mut Writer,
    snapshot: &economics::SubsidySnapshot,
) -> EncodeResult<()> {
    writer.write_u64(4);
    writer.write_string("storage_share_bps");
    writer.write_u64(snapshot.storage_share_bps as u64);
    writer.write_string("compute_share_bps");
    writer.write_u64(snapshot.compute_share_bps as u64);
    writer.write_string("energy_share_bps");
    writer.write_u64(snapshot.energy_share_bps as u64);
    writer.write_string("ad_share_bps");
    writer.write_u64(snapshot.ad_share_bps as u64);
    Ok(())
}

fn read_subsidy_snapshot(
    reader: &mut Reader<'_>,
) -> binary_struct::Result<economics::SubsidySnapshot> {
    let mut storage_share_bps = None;
    let mut compute_share_bps = None;
    let mut energy_share_bps = None;
    let mut ad_share_bps = None;
    decode_struct(reader, Some(4), |key, reader| match key {
        "storage_share_bps" => assign_once(
            &mut storage_share_bps,
            reader.read_u64()?,
            "storage_share_bps",
        ),
        "compute_share_bps" => assign_once(
            &mut compute_share_bps,
            reader.read_u64()?,
            "compute_share_bps",
        ),
        "energy_share_bps" => assign_once(
            &mut energy_share_bps,
            reader.read_u64()?,
            "energy_share_bps",
        ),
        "ad_share_bps" => assign_once(&mut ad_share_bps, reader.read_u64()?, "ad_share_bps"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(economics::SubsidySnapshot {
        storage_share_bps: storage_share_bps.unwrap_or_default() as u16,
        compute_share_bps: compute_share_bps.unwrap_or_default() as u16,
        energy_share_bps: energy_share_bps.unwrap_or_default() as u16,
        ad_share_bps: ad_share_bps.unwrap_or_default() as u16,
    })
}

fn write_tariff_snapshot(
    writer: &mut Writer,
    snapshot: &economics::TariffSnapshot,
) -> EncodeResult<()> {
    writer.write_u64(3);
    writer.write_string("tariff_bps");
    writer.write_u64(snapshot.tariff_bps as u64);
    writer.write_string("non_kyc_volume_block");
    writer.write_u64(snapshot.non_kyc_volume_block);
    writer.write_string("treasury_contribution_bps");
    writer.write_u64(snapshot.treasury_contribution_bps as u64);
    Ok(())
}

fn read_tariff_snapshot(
    reader: &mut Reader<'_>,
) -> binary_struct::Result<economics::TariffSnapshot> {
    let mut tariff_bps = None;
    let mut non_kyc_volume_block = None;
    let mut treasury_contribution_bps = None;
    decode_struct(reader, Some(3), |key, reader| match key {
        "tariff_bps" => assign_once(&mut tariff_bps, reader.read_u64()?, "tariff_bps"),
        "non_kyc_volume_block" => assign_once(
            &mut non_kyc_volume_block,
            reader.read_u64()?,
            "non_kyc_volume_block",
        ),
        "treasury_contribution_bps" => assign_once(
            &mut treasury_contribution_bps,
            reader.read_u64()?,
            "treasury_contribution_bps",
        ),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(economics::TariffSnapshot {
        tariff_bps: tariff_bps.unwrap_or_default() as u16,
        non_kyc_volume_block: non_kyc_volume_block.unwrap_or_default(),
        treasury_contribution_bps: treasury_contribution_bps.unwrap_or_default() as u16,
    })
}

/// Round f64 to 6 decimal places for deterministic serialization.
/// This ensures cross-platform consistency and avoids floating-point drift.
fn round_f64(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn write_market_metric(writer: &mut Writer, metric: &economics::MarketMetric) -> EncodeResult<()> {
    writer.write_u64(4);
    writer.write_string("utilization");
    writer.write_f64(round_f64(metric.utilization));
    writer.write_string("average_cost_block");
    writer.write_f64(round_f64(metric.average_cost_block));
    writer.write_string("effective_payout_block");
    writer.write_f64(round_f64(metric.effective_payout_block));
    writer.write_string("provider_margin");
    writer.write_f64(round_f64(metric.provider_margin));
    Ok(())
}

fn read_market_metric(reader: &mut Reader<'_>) -> binary_struct::Result<economics::MarketMetric> {
    let mut utilization = None;
    let mut average_cost_block = None;
    let mut effective_payout_block = None;
    let mut provider_margin = None;
    decode_struct(reader, Some(4), |key, reader| match key {
        "utilization" => assign_once(
            &mut utilization,
            round_f64(reader.read_f64()?),
            "utilization",
        ),
        "average_cost_block" => assign_once(
            &mut average_cost_block,
            round_f64(reader.read_f64()?),
            "average_cost_block",
        ),
        "effective_payout_block" => assign_once(
            &mut effective_payout_block,
            round_f64(reader.read_f64()?),
            "effective_payout_block",
        ),
        "provider_margin" => assign_once(
            &mut provider_margin,
            round_f64(reader.read_f64()?),
            "provider_margin",
        ),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(economics::MarketMetric {
        utilization: utilization.unwrap_or_default(),
        average_cost_block: average_cost_block.unwrap_or_default(),
        effective_payout_block: effective_payout_block.unwrap_or_default(),
        provider_margin: provider_margin.unwrap_or_default(),
    })
}

fn write_market_metrics(
    writer: &mut Writer,
    metrics: &economics::MarketMetrics,
) -> EncodeResult<()> {
    writer.write_u64(4);
    writer.write_string("storage");
    write_market_metric(writer, &metrics.storage)?;
    writer.write_string("compute");
    write_market_metric(writer, &metrics.compute)?;
    writer.write_string("energy");
    write_market_metric(writer, &metrics.energy)?;
    writer.write_string("ad");
    write_market_metric(writer, &metrics.ad)?;
    Ok(())
}

fn read_market_metrics(reader: &mut Reader<'_>) -> binary_struct::Result<economics::MarketMetrics> {
    let mut storage = None;
    let mut compute = None;
    let mut energy = None;
    let mut ad = None;
    decode_struct(reader, Some(4), |key, reader| match key {
        "storage" => assign_once(&mut storage, read_market_metric(reader)?, "storage"),
        "compute" => assign_once(&mut compute, read_market_metric(reader)?, "compute"),
        "energy" => assign_once(&mut energy, read_market_metric(reader)?, "energy"),
        "ad" => assign_once(&mut ad, read_market_metric(reader)?, "ad"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(economics::MarketMetrics {
        storage: storage.unwrap_or_default(),
        compute: compute.unwrap_or_default(),
        energy: energy.unwrap_or_default(),
        ad: ad.unwrap_or_default(),
    })
}

fn write_macro_block(writer: &mut Writer, block: &MacroBlock) -> EncodeResult<()> {
    writer.write_u64(4);
    writer.write_string("height");
    writer.write_u64(block.height);
    writer.write_string("shard_heights");
    write_u64_map(writer, &block.shard_heights)?;
    writer.write_string("shard_roots");
    write_root_map(writer, &block.shard_roots)?;
    writer.write_string("total_reward");
    writer.write_u64(block.total_reward);
    writer.write_string("queue_root");
    write_fixed(writer, &block.queue_root);
    Ok(())
}

fn read_macro_block(reader: &mut Reader<'_>) -> binary_struct::Result<MacroBlock> {
    let mut height = None;
    let mut shard_heights = None;
    let mut shard_roots = None;
    let mut total_reward = None;
    let mut queue_root = None;

    decode_struct(reader, Some(4), |key, reader| match key {
        "height" => assign_once(&mut height, reader.read_u64()?, "height"),
        "shard_heights" => assign_once(&mut shard_heights, read_u64_map(reader)?, "shard_heights"),
        "shard_roots" => assign_once(&mut shard_roots, read_root_map(reader)?, "shard_roots"),
        "total_reward" => assign_once(&mut total_reward, reader.read_u64()?, "total_reward"),
        "queue_root" => assign_once(&mut queue_root, read_fixed(reader)?, "queue_root"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(MacroBlock {
        height: height.unwrap_or_default(),
        shard_heights: shard_heights.unwrap_or_default(),
        shard_roots: shard_roots.unwrap_or_default(),
        total_reward: total_reward.unwrap_or_default(),
        queue_root: queue_root.unwrap_or([0; 32]),
    })
}

fn write_assist_receipt(writer: &mut Writer, receipt: &AssistReceipt) -> EncodeResult<()> {
    writer.write_u64(7);
    writer.write_string("provider");
    writer.write_string(&receipt.provider);
    writer.write_string("region");
    writer.write_string(&receipt.region);
    writer.write_string("pubkey");
    write_bytes(writer, &receipt.pubkey, "pubkey")?;
    writer.write_string("sig");
    write_bytes(writer, &receipt.sig, "sig")?;
    writer.write_string("device");
    writer.write_u8(receipt.device as u8);
    writer.write_string("rssi");
    writer.write_i64(i64::from(receipt.rssi));
    writer.write_string("rtt_ms");
    writer.write_u32(receipt.rtt_ms);
    Ok(())
}

fn write_u64_vec(writer: &mut Writer, values: &[u64], field: &'static str) -> EncodeResult<()> {
    write_vec(writer, values, field, |writer, value| {
        writer.write_u64(*value);
        Ok(())
    })
}

fn read_u64_vec(reader: &mut Reader<'_>, field: &'static str) -> binary_struct::Result<Vec<u64>> {
    read_vec(reader, field, |reader| {
        reader.read_u64().map_err(DecodeError::from)
    })
}

fn write_u64_map(writer: &mut Writer, map: &HashMap<u16, u64>) -> EncodeResult<()> {
    let mut entries: BTreeMap<u16, u64> = BTreeMap::new();
    for (&k, &v) in map {
        entries.insert(k, v);
    }
    let len =
        u64::try_from(entries.len()).map_err(|_| EncodeError::LengthOverflow("shard_heights"))?;
    writer.write_u64(len);
    for (k, v) in entries {
        writer.write_u16(k);
        writer.write_u64(v);
    }
    Ok(())
}

fn read_u64_map(reader: &mut Reader<'_>) -> binary_struct::Result<HashMap<u16, u64>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut map = HashMap::with_capacity(len);
    for _ in 0..len {
        let key = reader.read_u16()?;
        let value = reader.read_u64()?;
        map.insert(key, value);
    }
    Ok(map)
}

fn write_root_map(writer: &mut Writer, map: &HashMap<u16, [u8; 32]>) -> EncodeResult<()> {
    let mut entries: BTreeMap<u16, [u8; 32]> = BTreeMap::new();
    for (&k, v) in map {
        entries.insert(k, *v);
    }
    let len =
        u64::try_from(entries.len()).map_err(|_| EncodeError::LengthOverflow("shard_roots"))?;
    writer.write_u64(len);
    for (k, v) in entries {
        writer.write_u16(k);
        write_fixed(writer, &v);
    }
    Ok(())
}

fn read_root_map(reader: &mut Reader<'_>) -> binary_struct::Result<HashMap<u16, [u8; 32]>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut map = HashMap::with_capacity(len);
    for _ in 0..len {
        let key = reader.read_u16()?;
        let value = read_fixed(reader)?;
        map.insert(key, value);
    }
    Ok(map)
}

fn write_fixed(writer: &mut Writer, value: &[u8; 32]) {
    writer.write_u64(32);
    for byte in value {
        writer.write_u8(*byte);
    }
}

fn read_fixed(reader: &mut Reader<'_>) -> binary_struct::Result<[u8; 32]> {
    let len = reader.read_u64()?;
    if len != 32 {
        return Err(DecodeError::InvalidFieldValue {
            field: "fixed_array",
            reason: format!("expected length 32 got {len}"),
        });
    }
    let bytes = reader.read_exact(32)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes);
    Ok(out)
}

fn write_bytes(writer: &mut Writer, value: &[u8], field: &'static str) -> EncodeResult<()> {
    let _ = u64::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_bytes(value);
    Ok(())
}

fn write_vec<T, F>(
    writer: &mut Writer,
    values: &[T],
    field: &'static str,
    mut write: F,
) -> EncodeResult<()>
where
    F: FnMut(&mut Writer, &T) -> EncodeResult<()>,
{
    let len = u64::try_from(values.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_u64(len);
    for value in values {
        write(writer, value)?;
    }
    Ok(())
}

fn read_vec<T, F>(
    reader: &mut Reader<'_>,
    field: &'static str,
    mut read: F,
) -> binary_struct::Result<Vec<T>>
where
    F: FnMut(&mut Reader<'_>) -> binary_struct::Result<T>,
{
    let len_raw = reader.read_u64()?;
    let len = usize::try_from(len_raw).map_err(|_| DecodeError::InvalidFieldValue {
        field,
        reason: format!("length {len_raw} exceeds usize"),
    })?;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        out.push(read(reader)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{FeeLane, RawTxPayload, SignedTransaction, TxSignature, TxVersion};
    use crate::{Block, TokenAmount};
    use foundation_serialization::binary_cursor::{Reader, Writer};
    use std::collections::{HashMap, HashSet};

    fn sample_tx() -> SignedTransaction {
        SignedTransaction {
            payload: RawTxPayload {
                from_: "alice".into(),
                to: "bob".into(),
                amount_consumer: 1,
                amount_industrial: 2,
                fee: 3,
                pct: 50,
                nonce: 7,
                memo: vec![1, 2, 3],
            },
            public_key: vec![1, 2, 3, 4],
            #[cfg(feature = "quantum")]
            dilithium_public_key: vec![9, 9],
            signature: TxSignature {
                ed25519: vec![5, 6],
                #[cfg(feature = "quantum")]
                dilithium: vec![7, 8],
            },
            tip: 11,
            signer_pubkeys: vec![vec![1], vec![2, 2]],
            aggregate_signature: vec![0xaa, 0xbb],
            threshold: 1,
            lane: FeeLane::Consumer,
            version: TxVersion::Ed25519Only,
        }
    }

    fn sample_block() -> Block {
        Block {
            index: 1,
            previous_hash: String::from("prev"),
            timestamp_millis: 42,
            transactions: vec![sample_tx()],
            difficulty: 9,
            retune_hint: 0,
            nonce: 7,
            hash: String::from("hash"),
            coinbase_block: TokenAmount::new(11),
            coinbase_industrial: TokenAmount::new(12),
            storage_sub: TokenAmount::new(13),
            read_sub: TokenAmount::new(14),
            read_sub_viewer: TokenAmount::new(2),
            read_sub_host: TokenAmount::new(3),
            read_sub_hardware: TokenAmount::new(4),
            read_sub_verifier: TokenAmount::new(1),
            read_sub_liquidity: TokenAmount::new(2),
            ad_viewer: TokenAmount::new(5),
            ad_host: TokenAmount::new(6),
            ad_hardware: TokenAmount::new(7),
            ad_verifier: TokenAmount::new(8),
            ad_liquidity: TokenAmount::new(9),
            ad_miner: TokenAmount::new(10),
            treasury_events: Vec::new(),
            ad_total_usd_micros: 26,
            ad_settlement_count: 2,
            ad_oracle_price_usd_micros: 27,
            compute_sub: TokenAmount::new(15),
            proof_rebate: TokenAmount::new(16),
            read_root: [0; 32],
            fee_checksum: String::new(),
            state_root: String::new(),
            base_fee: 21,
            l2_roots: vec![],
            l2_sizes: vec![],
            vdf_commit: [0; 32],
            vdf_output: [0; 32],
            vdf_proof: vec![],
            #[cfg(feature = "quantum")]
            dilithium_pubkey: vec![],
            #[cfg(feature = "quantum")]
            dilithium_sig: vec![],
            receipts: Vec::new(),
        }
    }

    #[test]
    fn account_roundtrip() {
        let account = Account {
            address: "alice".into(),
            balance: TokenBalance {
                amount: 30,  // 10+20 in single BLOCK token
            },
            nonce: 3,
            pending_amount: 9,  // 4+5 in single BLOCK token
            pending_nonce: 6,
            pending_nonces: [1, 2, 3].into_iter().collect(),
            sessions: vec![SessionPolicy {
                public_key: vec![1, 2, 3],
                expires_at: 99,
                nonce: 7,
            }],
        };
        let bytes = encode_account(&account).expect("encode");
        let decoded = decode_account(&bytes).expect("decode");
        assert_eq!(decoded, account);
    }

    #[test]
    fn decode_block_vec_roundtrip() {
        let block = sample_block();
        let mut writer = Writer::with_capacity(512);
        writer.write_u64(1);
        block_binary::write_block(&mut writer, &block).expect("encode block");
        let bytes = writer.finish();

        let decoded = decode_block_vec(&bytes).expect("decode blocks");
        assert_eq!(decoded, vec![block]);
    }

    #[test]
    fn decode_account_map_bytes_roundtrip() {
        let account = Account {
            address: "alice".into(),
            balance: TokenBalance {
                amount: 3,  // 1+2 in single BLOCK token
            },
            nonce: 3,
            pending_amount: 9,  // 4+5 in single BLOCK token
            pending_nonce: 6,
            pending_nonces: HashSet::from([1, 2, 3]),
            sessions: vec![],
        };
        let mut accounts = HashMap::new();
        accounts.insert(String::from("alice"), account.clone());

        let mut writer = Writer::with_capacity(512);
        write_account_map(&mut writer, &accounts).expect("encode map");
        let bytes = writer.finish();

        let decoded = decode_account_map_bytes(&bytes).expect("decode map");
        assert_eq!(decoded, accounts);
    }

    #[test]
    fn decode_emission_tuple_roundtrip() {
        let mut writer = Writer::with_capacity(128);
        writer.write_u64(5);
        writer.write_string("emission_consumer");
        writer.write_u64(1);
        writer.write_string("emission_industrial");
        writer.write_u64(2);
        writer.write_string("block_reward_consumer");
        writer.write_u64(3);
        writer.write_string("block_reward_industrial");
        writer.write_u64(4);
        writer.write_string("block_height");
        writer.write_u64(5);
        let bytes = writer.finish();

        let decoded = decode_emission_tuple(&bytes).expect("decode tuple");
        assert_eq!(decoded, (1, 2, 3, 4, 5));
    }

    #[test]
    fn mempool_entry_defaults_serialized_size_when_missing() {
        let tx = sample_tx();
        let mut writer = Writer::with_capacity(512);
        writer.write_u64(5);
        writer.write_string("sender");
        writer.write_string("alice");
        writer.write_string("nonce");
        writer.write_u64(1);
        writer.write_string("tx");
        tx_binary::write_signed_transaction(&mut writer, &tx).expect("encode tx");
        writer.write_string("timestamp_millis");
        writer.write_u64(2);
        writer.write_string("timestamp_ticks");
        writer.write_u64(3);
        let bytes = writer.finish();

        let mut reader = Reader::new(&bytes);
        let entry = read_mempool_entry(&mut reader).expect("decode entry");
        ensure_exhausted(&reader).expect("exhausted");

        assert_eq!(entry.sender, "alice");
        assert_eq!(entry.nonce, 1);
        assert_eq!(entry.serialized_size, 0);
    }

    #[test]
    fn chain_disk_roundtrip() {
        let account = Account {
            address: "alice".into(),
            balance: TokenBalance {
                amount: 3,  // 1+2 in single BLOCK token
            },
            nonce: 3,
            pending_amount: 9,  // 4+5 in single BLOCK token
            pending_nonce: 6,
            pending_nonces: HashSet::from([1, 2, 3]),
            sessions: vec![],
        };
        let block = sample_block();
        let tx = sample_tx();
        let serialized_size = tx_binary::encode_signed_transaction(&tx)
            .expect("encode tx")
            .len() as u64;
        let disk = ChainDisk {
            schema_version: 9,
            chain: vec![block],
            accounts: HashMap::from([(String::from("alice"), account.clone())]),
            emission: 15, // Was: emission_consumer: 7 + emission_industrial: 8
            emission_year_ago: 9,
            inflation_epoch_marker: 10,
            block_reward: TokenAmount::new(23), // Was: block_reward_consumer: 11 + block_reward_industrial: 12
            block_height: 13,
            mempool: vec![MempoolEntryDisk {
                sender: "alice".into(),
                nonce: 1,
                tx: tx,
                timestamp_millis: 2,
                timestamp_ticks: 3,
                serialized_size,
            }],
            base_fee: 14,
            params: Params::default(),
            epoch_storage_bytes: 15,
            epoch_read_bytes: 16,
            epoch_cpu_ms: 17,
            epoch_bytes_out: 18,
            recent_timestamps: vec![19, 20],
            economics_block_reward_per_block: 21,
            economics_prev_annual_issuance_block: 22,
            economics_prev_subsidy: crate::economics::SubsidySnapshot::default(),
            economics_prev_tariff: crate::economics::TariffSnapshot::default(),
            economics_epoch_tx_volume_block: 23,
            economics_epoch_tx_count: 24,
            economics_epoch_treasury_inflow_block: 25,
            economics_epoch_storage_payout_block: 1000,
            economics_epoch_compute_payout_block: 2000,
            economics_epoch_ad_payout_block: 3000,
            economics_baseline_tx_count: 100,
            economics_baseline_tx_volume: 10_000,
            economics_baseline_miners: 10,
            economics_prev_market_metrics: crate::economics::MarketMetrics::default(),
        };
        let bytes = encode_chain_disk(&disk).expect("encode");
        let decoded = decode_chain_disk(&bytes).expect("decode");
        assert_eq!(decoded.schema_version, disk.schema_version);
        assert_eq!(decoded.accounts.get("alice"), Some(&account));
        assert_eq!(decoded.emission, disk.emission);
        assert_eq!(decoded.base_fee, disk.base_fee);
        assert_eq!(decoded.recent_timestamps, disk.recent_timestamps);
        assert_eq!(
            decoded.mempool.first().map(|e| e.serialized_size),
            Some(serialized_size)
        );
    }
}
