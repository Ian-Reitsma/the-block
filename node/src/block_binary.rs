use std::collections::HashMap;
use std::convert::TryFrom;

use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::receipts_validation::{ReceiptAggregateScheme, ReceiptHeader};
use crate::transaction::binary as tx_binary;
use crate::transaction::binary::{EncodeError, EncodeResult};
use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};
use crate::{
    AdReceipt, Block, BlockTreasuryEvent, ComputeReceipt, EnergyReceipt, Receipt,
    SignedTransaction, StorageReceipt, TokenAmount,
};

/// Encode a [`Block`] into the canonical binary layout.
pub fn encode_block(block: &Block) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(1024);
    write_block(&mut writer, block)?;
    Ok(writer.finish())
}

/// Decode a [`Block`] produced by [`encode_block`].
pub fn decode_block(bytes: &[u8]) -> binary_struct::Result<Block> {
    let mut reader = Reader::new(bytes);
    let block = read_block(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(block)
}

pub(crate) fn write_block(writer: &mut Writer, block: &Block) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_u64("index", block.index);
        struct_writer.field_string("previous_hash", &block.previous_hash);
        struct_writer.field_u64("timestamp_millis", block.timestamp_millis);
        struct_writer.field_with("transactions", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_transactions(field_writer, &block.transactions) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_u64("difficulty", block.difficulty);
        struct_writer.field_i64("retune_hint", i64::from(block.retune_hint));
        struct_writer.field_u64("nonce", block.nonce);
        struct_writer.field_string("hash", &block.hash);
        struct_writer.field_u64("coinbase_block", block.coinbase_block.get());
        struct_writer.field_u64("coinbase_industrial", block.coinbase_industrial.get());
        struct_writer.field_u64("storage_sub", block.storage_sub.get());
        struct_writer.field_u64("read_sub", block.read_sub.get());
        struct_writer.field_u64("read_sub_viewer", block.read_sub_viewer.get());
        struct_writer.field_u64("read_sub_host", block.read_sub_host.get());
        struct_writer.field_u64("read_sub_hardware", block.read_sub_hardware.get());
        struct_writer.field_u64("read_sub_verifier", block.read_sub_verifier.get());
        struct_writer.field_u64("read_sub_liquidity", block.read_sub_liquidity.get());
        struct_writer.field_u64("ad_viewer", block.ad_viewer.get());
        struct_writer.field_u64("ad_host", block.ad_host.get());
        struct_writer.field_u64("ad_hardware", block.ad_hardware.get());
        struct_writer.field_u64("ad_verifier", block.ad_verifier.get());
        struct_writer.field_u64("ad_liquidity", block.ad_liquidity.get());
        struct_writer.field_u64("ad_miner", block.ad_miner.get());
        struct_writer.field_with("treasury_events", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_treasury_events(field_writer, &block.treasury_events) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_u64("ad_total_usd_micros", block.ad_total_usd_micros);
        struct_writer.field_u64("ad_settlement_count", block.ad_settlement_count);
        struct_writer.field_u64(
            "ad_oracle_price_usd_micros",
            block.ad_oracle_price_usd_micros,
        );
        struct_writer.field_u64("compute_sub", block.compute_sub.get());
        struct_writer.field_u64("proof_rebate", block.proof_rebate.get());
        struct_writer.field_with("read_root", |field_writer| {
            write_fixed(field_writer, &block.read_root);
        });
        struct_writer.field_string("fee_checksum", &block.fee_checksum);
        struct_writer.field_string("state_root", &block.state_root);
        struct_writer.field_u64("base_fee", block.base_fee);
        struct_writer.field_with("l2_roots", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_root_vec(field_writer, &block.l2_roots) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("l2_sizes", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_u32_vec(field_writer, &block.l2_sizes) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("vdf_commit", |field_writer| {
            write_fixed(field_writer, &block.vdf_commit);
        });
        struct_writer.field_with("vdf_output", |field_writer| {
            write_fixed(field_writer, &block.vdf_output);
        });
        struct_writer.field_with("vdf_proof", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_bytes(field_writer, &block.vdf_proof, "vdf_proof") {
                    result = Err(err);
                }
            }
        });
        #[cfg(feature = "quantum")]
        {
            struct_writer.field_with("dilithium_pubkey", |field_writer| {
                if result.is_ok() {
                    if let Err(err) =
                        write_bytes(field_writer, &block.dilithium_pubkey, "dilithium_pubkey")
                    {
                        result = Err(err);
                    }
                }
            });
            struct_writer.field_with("dilithium_sig", |field_writer| {
                if result.is_ok() {
                    if let Err(err) =
                        write_bytes(field_writer, &block.dilithium_sig, "dilithium_sig")
                    {
                        result = Err(err);
                    }
                }
            });
        }
        struct_writer.field_with("receipt_header", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_optional_receipt_header(field_writer, &block.receipt_header)
                {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("receipts", |field_writer| {
            if result.is_ok() {
                if let Err(err) = write_receipts(field_writer, &block.receipts) {
                    result = Err(err);
                }
            }
        });
    });
    result
}

pub(crate) fn read_block(reader: &mut Reader<'_>) -> binary_struct::Result<Block> {
    let mut index = None;
    let mut previous_hash = None;
    let mut timestamp_millis = None;
    let mut transactions: Option<Vec<SignedTransaction>> = None;
    let mut difficulty = None;
    let mut retune_hint = None;
    let mut nonce = None;
    let mut hash = None;
    let mut coinbase_block = None;
    let mut coinbase_industrial = None;
    let mut storage_sub = None;
    let mut read_sub = None;
    let mut read_sub_viewer = None;
    let mut read_sub_host = None;
    let mut read_sub_hardware = None;
    let mut read_sub_verifier = None;
    let mut read_sub_liquidity = None;
    let mut ad_viewer = None;
    let mut ad_host = None;
    let mut ad_hardware = None;
    let mut ad_verifier = None;
    let mut ad_liquidity = None;
    let mut ad_miner = None;
    let mut treasury_events: Option<Vec<BlockTreasuryEvent>> = None;
    let mut ad_total_usd_micros = None;
    let mut ad_settlement_count = None;
    let mut ad_oracle_price_usd_micros = None;
    let mut compute_sub = None;
    let mut proof_rebate = None;
    let mut read_root = None;
    let mut fee_checksum = None;
    let mut state_root = None;
    let mut base_fee = None;
    let mut l2_roots = None;
    let mut l2_sizes = None;
    let mut vdf_commit = None;
    let mut vdf_output = None;
    let mut vdf_proof = None;
    #[cfg(feature = "quantum")]
    let mut dilithium_pubkey = None;
    #[cfg(feature = "quantum")]
    let mut dilithium_sig = None;
    let mut receipt_header = None;
    let mut receipts = None;

    decode_struct(reader, None, |key, reader| match key {
        "index" => assign_once(&mut index, reader.read_u64()?, "index"),
        "previous_hash" => assign_once(&mut previous_hash, reader.read_string()?, "previous_hash"),
        "timestamp_millis" => assign_once(
            &mut timestamp_millis,
            reader.read_u64()?,
            "timestamp_millis",
        ),
        "transactions" => assign_once(
            &mut transactions,
            read_transactions(reader)?,
            "transactions",
        ),
        "difficulty" => assign_once(&mut difficulty, reader.read_u64()?, "difficulty"),
        "retune_hint" => assign_once(&mut retune_hint, read_retune_hint(reader)?, "retune_hint"),
        "nonce" => assign_once(&mut nonce, reader.read_u64()?, "nonce"),
        "hash" => assign_once(&mut hash, reader.read_string()?, "hash"),
        "coinbase_block" => assign_once(&mut coinbase_block, reader.read_u64()?, "coinbase_block"),
        "coinbase_industrial" => assign_once(
            &mut coinbase_industrial,
            reader.read_u64()?,
            "coinbase_industrial",
        ),
        "storage_sub" => assign_once(&mut storage_sub, reader.read_u64()?, "storage_sub"),
        "read_sub" => assign_once(&mut read_sub, reader.read_u64()?, "read_sub"),
        "read_sub_viewer" => {
            assign_once(&mut read_sub_viewer, reader.read_u64()?, "read_sub_viewer")
        }
        "read_sub_host" => assign_once(&mut read_sub_host, reader.read_u64()?, "read_sub_host"),
        "read_sub_hardware" => assign_once(
            &mut read_sub_hardware,
            reader.read_u64()?,
            "read_sub_hardware",
        ),
        "read_sub_verifier" => assign_once(
            &mut read_sub_verifier,
            reader.read_u64()?,
            "read_sub_verifier",
        ),
        "read_sub_liquidity" => assign_once(
            &mut read_sub_liquidity,
            reader.read_u64()?,
            "read_sub_liquidity",
        ),
        "ad_viewer" => assign_once(&mut ad_viewer, reader.read_u64()?, "ad_viewer"),
        "ad_host" => assign_once(&mut ad_host, reader.read_u64()?, "ad_host"),
        "ad_hardware" => assign_once(&mut ad_hardware, reader.read_u64()?, "ad_hardware"),
        "ad_verifier" => assign_once(&mut ad_verifier, reader.read_u64()?, "ad_verifier"),
        "ad_liquidity" => assign_once(&mut ad_liquidity, reader.read_u64()?, "ad_liquidity"),
        "ad_miner" => assign_once(&mut ad_miner, reader.read_u64()?, "ad_miner"),
        "treasury_events" => assign_once(
            &mut treasury_events,
            read_treasury_events(reader)?,
            "treasury_events",
        ),
        "ad_total_usd_micros" => assign_once(
            &mut ad_total_usd_micros,
            reader.read_u64()?,
            "ad_total_usd_micros",
        ),
        "ad_settlement_count" => assign_once(
            &mut ad_settlement_count,
            reader.read_u64()?,
            "ad_settlement_count",
        ),
        "ad_oracle_price_usd_micros" => assign_once(
            &mut ad_oracle_price_usd_micros,
            reader.read_u64()?,
            "ad_oracle_price_usd_micros",
        ),
        "compute_sub" => assign_once(&mut compute_sub, reader.read_u64()?, "compute_sub"),
        "proof_rebate" => assign_once(&mut proof_rebate, reader.read_u64()?, "proof_rebate"),
        "read_root" => assign_once(&mut read_root, read_fixed(reader)?, "read_root"),
        "fee_checksum" => assign_once(&mut fee_checksum, reader.read_string()?, "fee_checksum"),
        "state_root" => assign_once(&mut state_root, reader.read_string()?, "state_root"),
        "base_fee" => assign_once(&mut base_fee, reader.read_u64()?, "base_fee"),
        "l2_roots" => assign_once(&mut l2_roots, read_root_vec(reader)?, "l2_roots"),
        "l2_sizes" => assign_once(&mut l2_sizes, read_u32_vec(reader)?, "l2_sizes"),
        "vdf_commit" => assign_once(&mut vdf_commit, read_fixed(reader)?, "vdf_commit"),
        "vdf_output" => assign_once(&mut vdf_output, read_fixed(reader)?, "vdf_output"),
        "vdf_proof" => assign_once(&mut vdf_proof, reader.read_bytes()?, "vdf_proof"),
        #[cfg(feature = "quantum")]
        "dilithium_pubkey" => assign_once(
            &mut dilithium_pubkey,
            reader.read_bytes()?,
            "dilithium_pubkey",
        ),
        #[cfg(feature = "quantum")]
        "dilithium_sig" => assign_once(&mut dilithium_sig, reader.read_bytes()?, "dilithium_sig"),
        "receipt_header" => assign_once(
            &mut receipt_header,
            read_optional_receipt_header(reader)?,
            "receipt_header",
        ),
        "receipts" => assign_once(&mut receipts, read_receipts(reader)?, "receipts"),
        other => Err(DecodeError::UnknownField(other.to_owned())),
    })?;

    Ok(Block {
        index: index.ok_or(DecodeError::MissingField("index"))?,
        previous_hash: previous_hash.ok_or(DecodeError::MissingField("previous_hash"))?,
        timestamp_millis: timestamp_millis.ok_or(DecodeError::MissingField("timestamp_millis"))?,
        transactions: transactions.unwrap_or_default(),
        difficulty: difficulty.unwrap_or_default(),
        retune_hint: retune_hint.unwrap_or_default(),
        nonce: nonce.ok_or(DecodeError::MissingField("nonce"))?,
        hash: hash.ok_or(DecodeError::MissingField("hash"))?,
        coinbase_block: TokenAmount::new(coinbase_block.unwrap_or_default()),
        coinbase_industrial: TokenAmount::new(coinbase_industrial.unwrap_or_default()),
        storage_sub: TokenAmount::new(storage_sub.unwrap_or_default()),
        read_sub: TokenAmount::new(read_sub.unwrap_or_default()),
        read_sub_viewer: TokenAmount::new(read_sub_viewer.unwrap_or_default()),
        read_sub_host: TokenAmount::new(read_sub_host.unwrap_or_default()),
        read_sub_hardware: TokenAmount::new(read_sub_hardware.unwrap_or_default()),
        read_sub_verifier: TokenAmount::new(read_sub_verifier.unwrap_or_default()),
        read_sub_liquidity: TokenAmount::new(read_sub_liquidity.unwrap_or_default()),
        ad_viewer: TokenAmount::new(ad_viewer.unwrap_or_default()),
        ad_host: TokenAmount::new(ad_host.unwrap_or_default()),
        ad_hardware: TokenAmount::new(ad_hardware.unwrap_or_default()),
        ad_verifier: TokenAmount::new(ad_verifier.unwrap_or_default()),
        ad_liquidity: TokenAmount::new(ad_liquidity.unwrap_or_default()),
        ad_miner: TokenAmount::new(ad_miner.unwrap_or_default()),
        treasury_events: treasury_events.unwrap_or_default(),
        ad_total_usd_micros: ad_total_usd_micros.unwrap_or_default(),
        ad_settlement_count: ad_settlement_count.unwrap_or_default(),
        ad_oracle_price_usd_micros: ad_oracle_price_usd_micros.unwrap_or_default(),
        compute_sub: TokenAmount::new(compute_sub.unwrap_or_default()),
        proof_rebate: TokenAmount::new(proof_rebate.unwrap_or_default()),
        read_root: read_root.unwrap_or([0; 32]),
        fee_checksum: fee_checksum.unwrap_or_default(),
        state_root: state_root.unwrap_or_default(),
        base_fee: base_fee.unwrap_or_default(),
        l2_roots: l2_roots.unwrap_or_default(),
        l2_sizes: l2_sizes.unwrap_or_default(),
        vdf_commit: vdf_commit.unwrap_or([0; 32]),
        vdf_output: vdf_output.unwrap_or([0; 32]),
        vdf_proof: vdf_proof.unwrap_or_default(),
        #[cfg(feature = "quantum")]
        dilithium_pubkey: dilithium_pubkey.unwrap_or_default(),
        #[cfg(feature = "quantum")]
        dilithium_sig: dilithium_sig.unwrap_or_default(),
        receipt_header: receipt_header.unwrap_or_default(),
        receipts: receipts.unwrap_or_default(),
    })
}

fn write_transactions(writer: &mut Writer, txs: &[SignedTransaction]) -> EncodeResult<()> {
    write_vec(writer, txs, "transactions", |writer, tx| {
        tx_binary::write_signed_transaction(writer, tx)
    })
}

fn read_transactions(reader: &mut Reader<'_>) -> Result<Vec<SignedTransaction>, DecodeError> {
    read_vec(reader, |reader| tx_binary::read_signed_transaction(reader))
}

fn write_treasury_events(writer: &mut Writer, events: &[BlockTreasuryEvent]) -> EncodeResult<()> {
    write_vec(writer, events, "treasury_events", |writer, event| {
        writer.write_struct(|struct_writer| {
            struct_writer.field_u64("disbursement_id", event.disbursement_id);
            struct_writer.field_string("destination", &event.destination);
            struct_writer.field_u64("amount", event.amount);
            struct_writer.field_string("memo", &event.memo);
            struct_writer.field_u64("scheduled_epoch", event.scheduled_epoch);
            struct_writer.field_string("tx_hash", &event.tx_hash);
            struct_writer.field_u64("executed_at", event.executed_at);
        });
        Ok(())
    })
}

fn read_treasury_events(reader: &mut Reader<'_>) -> Result<Vec<BlockTreasuryEvent>, DecodeError> {
    read_vec(reader, |reader| {
        let mut disbursement_id = None;
        let mut destination = None;
        let mut amount = None;
        let mut memo = None;
        let mut scheduled_epoch = None;
        let mut tx_hash = None;
        let mut executed_at = None;
        decode_struct(reader, Some(7), |key, reader| match key {
            "disbursement_id" => {
                assign_once(&mut disbursement_id, reader.read_u64()?, "disbursement_id")
            }
            "destination" => assign_once(&mut destination, reader.read_string()?, "destination"),
            "amount" => assign_once(&mut amount, reader.read_u64()?, "amount"),
            "memo" => assign_once(&mut memo, reader.read_string()?, "memo"),
            "scheduled_epoch" => {
                assign_once(&mut scheduled_epoch, reader.read_u64()?, "scheduled_epoch")
            }
            "tx_hash" => assign_once(&mut tx_hash, reader.read_string()?, "tx_hash"),
            "executed_at" => assign_once(&mut executed_at, reader.read_u64()?, "executed_at"),
            other => Err(DecodeError::UnknownField(other.to_string())),
        })?;
        Ok(BlockTreasuryEvent {
            disbursement_id: disbursement_id.unwrap_or_default(),
            destination: destination.unwrap_or_default(),
            amount: amount.unwrap_or_default(),
            memo: memo.unwrap_or_default(),
            scheduled_epoch: scheduled_epoch.unwrap_or_default(),
            tx_hash: tx_hash.unwrap_or_default(),
            executed_at: executed_at.unwrap_or_default(),
        })
    })
}

fn write_receipts(writer: &mut Writer, receipts: &[Receipt]) -> EncodeResult<()> {
    write_vec(writer, receipts, "receipts", |writer, receipt| {
        writer.write_struct(|struct_writer| match receipt {
            Receipt::Storage(r) => {
                struct_writer.field_string("type", "storage");
                struct_writer.field_string("contract_id", &r.contract_id);
                struct_writer.field_string("provider", &r.provider);
                struct_writer.field_u64("bytes", r.bytes);
                struct_writer.field_u64("price", r.price);
                struct_writer.field_u64("block_height", r.block_height);
                struct_writer.field_u64("provider_escrow", r.provider_escrow);
                struct_writer.field_with("provider_signature", |field_writer| {
                    write_bytes(field_writer, &r.provider_signature, "provider_signature")
                        .expect("signature length fits");
                });
                struct_writer.field_u64("signature_nonce", r.signature_nonce);
            }
            Receipt::Compute(r) => {
                struct_writer.field_string("type", "compute");
                struct_writer.field_string("job_id", &r.job_id);
                struct_writer.field_string("provider", &r.provider);
                struct_writer.field_u64("compute_units", r.compute_units);
                struct_writer.field_u64("payment", r.payment);
                struct_writer.field_u64("block_height", r.block_height);
                struct_writer.field_u64("verified", if r.verified { 1 } else { 0 });
                struct_writer.field_with("provider_signature", |field_writer| {
                    write_bytes(field_writer, &r.provider_signature, "provider_signature")
                        .expect("signature length fits");
                });
                struct_writer.field_u64("signature_nonce", r.signature_nonce);
            }
            Receipt::Energy(r) => {
                struct_writer.field_string("type", "energy");
                struct_writer.field_string("contract_id", &r.contract_id);
                struct_writer.field_string("provider", &r.provider);
                struct_writer.field_u64("energy_units", r.energy_units);
                struct_writer.field_u64("price", r.price);
                struct_writer.field_u64("block_height", r.block_height);
                struct_writer.field_with("proof_hash", |field_writer| {
                    write_fixed(field_writer, &r.proof_hash);
                });
                struct_writer.field_with("provider_signature", |field_writer| {
                    write_bytes(field_writer, &r.provider_signature, "provider_signature")
                        .expect("signature length fits");
                });
                struct_writer.field_u64("signature_nonce", r.signature_nonce);
            }
            Receipt::Ad(r) => {
                struct_writer.field_string("type", "ad");
                struct_writer.field_string("campaign_id", &r.campaign_id);
                struct_writer.field_string("creative_id", &r.creative_id);
                struct_writer.field_string("publisher", &r.publisher);
                struct_writer.field_u64("impressions", r.impressions);
                struct_writer.field_u64("spend", r.spend);
                struct_writer.field_u64("block_height", r.block_height);
                struct_writer.field_u64("conversions", r.conversions as u64);
                if !r.claim_routes.is_empty() {
                    let mut routes: Vec<_> = r.claim_routes.iter().collect();
                    routes.sort_by(|a, b| a.0.cmp(b.0));
                    struct_writer.field_with("claim_routes", |field_writer| {
                        write_vec(
                            field_writer,
                            &routes,
                            "claim_routes",
                            |writer, (role, addr)| {
                                writer.write_struct(|route_writer| {
                                    route_writer.field_string("role", role);
                                    route_writer.field_string("address", addr);
                                });
                                Ok(())
                            },
                        )
                        .expect("encode claim routes")
                    });
                }
                if let Some(breakdown) = r.role_breakdown.as_ref() {
                    struct_writer.field_with("role_breakdown", |field_writer| {
                        field_writer.write_struct(|rb_writer| {
                            rb_writer.field_u64("viewer", breakdown.viewer);
                            rb_writer.field_u64("host", breakdown.host);
                            rb_writer.field_u64("hardware", breakdown.hardware);
                            rb_writer.field_u64("verifier", breakdown.verifier);
                            rb_writer.field_u64("liquidity", breakdown.liquidity);
                            rb_writer.field_u64("miner", breakdown.miner);
                            rb_writer.field_u64("price_usd_micros", breakdown.price_usd_micros);
                            rb_writer.field_u64(
                                "clearing_price_usd_micros",
                                breakdown.clearing_price_usd_micros,
                            );
                        });
                    });
                }
                if !r.device_links.is_empty() {
                    let mut links = r.device_links.clone();
                    links.sort_by(|a, b| a.device_hash.cmp(&b.device_hash));
                    struct_writer.field_with("device_links", |field_writer| {
                        write_vec(field_writer, &links, "device_links", |writer, link| {
                            writer.write_struct(|link_writer| {
                                link_writer.field_string("device_hash", &link.device_hash);
                                link_writer.field_u64("opt_in", if link.opt_in { 1 } else { 0 });
                            });
                            Ok(())
                        })
                        .expect("encode device links")
                    });
                }
                struct_writer.field_with("publisher_signature", |field_writer| {
                    write_bytes(field_writer, &r.publisher_signature, "publisher_signature")
                        .expect("signature length fits");
                });
                struct_writer.field_u64("signature_nonce", r.signature_nonce);
            }
        });
        Ok(())
    })
}

fn read_receipts(reader: &mut Reader<'_>) -> Result<Vec<Receipt>, DecodeError> {
    read_vec(reader, |reader| {
        let mut receipt_type = None;
        let mut contract_id = None;
        let mut job_id = None;
        let mut campaign_id = None;
        let mut creative_id = None;
        let mut provider = None;
        let mut publisher = None;
        let mut bytes = None;
        let mut compute_units = None;
        let mut energy_units = None;
        let mut impressions = None;
        let mut price = None;
        let mut payment = None;
        let mut spend = None;
        let mut block_height = None;
        let mut provider_escrow = None;
        let mut verified = None;
        let mut proof_hash = None;
        let mut conversions = None;
        let mut claim_routes: Option<HashMap<String, String>> = None;
        let mut role_breakdown: Option<crate::receipts::AdRoleBreakdown> = None;
        let mut device_links: Option<Vec<ad_market::DeviceLinkOptIn>> = None;
        let mut provider_signature = None;
        let mut publisher_signature = None;
        let mut signature_nonce = None;

        decode_struct(reader, None, |key, reader| match key {
            "type" => assign_once(&mut receipt_type, reader.read_string()?, "type"),
            "contract_id" => assign_once(&mut contract_id, reader.read_string()?, "contract_id"),
            "job_id" => assign_once(&mut job_id, reader.read_string()?, "job_id"),
            "campaign_id" => assign_once(&mut campaign_id, reader.read_string()?, "campaign_id"),
            "creative_id" => assign_once(&mut creative_id, reader.read_string()?, "creative_id"),
            "provider" => assign_once(&mut provider, reader.read_string()?, "provider"),
            "publisher" => assign_once(&mut publisher, reader.read_string()?, "publisher"),
            "bytes" => assign_once(&mut bytes, reader.read_u64()?, "bytes"),
            "compute_units" => assign_once(&mut compute_units, reader.read_u64()?, "compute_units"),
            "energy_units" => assign_once(&mut energy_units, reader.read_u64()?, "energy_units"),
            "impressions" => assign_once(&mut impressions, reader.read_u64()?, "impressions"),
            "price" => assign_once(&mut price, reader.read_u64()?, "price"),
            "payment" => assign_once(&mut payment, reader.read_u64()?, "payment"),
            "spend" => assign_once(&mut spend, reader.read_u64()?, "spend"),
            "block_height" => assign_once(&mut block_height, reader.read_u64()?, "block_height"),
            "provider_escrow" => {
                assign_once(&mut provider_escrow, reader.read_u64()?, "provider_escrow")
            }
            "verified" => assign_once(&mut verified, reader.read_u64()?, "verified"),
            "proof_hash" => assign_once(&mut proof_hash, read_fixed(reader)?, "proof_hash"),
            "conversions" => assign_once(&mut conversions, reader.read_u64()?, "conversions"),
            "claim_routes" => {
                let routes = read_vec(reader, |reader| {
                    let mut role = None;
                    let mut address = None;
                    decode_struct(reader, Some(2), |key, reader| match key {
                        "role" => assign_once(&mut role, reader.read_string()?, "role"),
                        "address" => assign_once(&mut address, reader.read_string()?, "address"),
                        other => Err(DecodeError::UnknownField(other.to_string())),
                    })?;
                    Ok((
                        role.ok_or(DecodeError::MissingField("role"))?,
                        address.ok_or(DecodeError::MissingField("address"))?,
                    ))
                })?;
                let mut map = HashMap::with_capacity(routes.len());
                for (role, address) in routes {
                    map.insert(role, address);
                }
                assign_once(&mut claim_routes, map, "claim_routes")
            }
            "role_breakdown" => {
                let mut viewer = None;
                let mut host = None;
                let mut hardware = None;
                let mut verifier = None;
                let mut liquidity = None;
                let mut miner = None;
                let mut price_usd_micros = None;
                let mut clearing_price_usd_micros = None;
                decode_struct(reader, None, |key, reader| match key {
                    "viewer" => assign_once(&mut viewer, reader.read_u64()?, "viewer"),
                    "host" => assign_once(&mut host, reader.read_u64()?, "host"),
                    "hardware" => assign_once(&mut hardware, reader.read_u64()?, "hardware"),
                    "verifier" => assign_once(&mut verifier, reader.read_u64()?, "verifier"),
                    "liquidity" => assign_once(&mut liquidity, reader.read_u64()?, "liquidity"),
                    "miner" => assign_once(&mut miner, reader.read_u64()?, "miner"),
                    "price_usd_micros" => assign_once(
                        &mut price_usd_micros,
                        reader.read_u64()?,
                        "price_usd_micros",
                    ),
                    "clearing_price_usd_micros" => assign_once(
                        &mut clearing_price_usd_micros,
                        reader.read_u64()?,
                        "clearing_price_usd_micros",
                    ),
                    other => Err(DecodeError::UnknownField(other.to_string())),
                })?;
                assign_once(
                    &mut role_breakdown,
                    crate::receipts::AdRoleBreakdown {
                        viewer: viewer.unwrap_or_default(),
                        host: host.unwrap_or_default(),
                        hardware: hardware.unwrap_or_default(),
                        verifier: verifier.unwrap_or_default(),
                        liquidity: liquidity.unwrap_or_default(),
                        miner: miner.unwrap_or_default(),
                        price_usd_micros: price_usd_micros.unwrap_or_default(),
                        clearing_price_usd_micros: clearing_price_usd_micros.unwrap_or_default(),
                    },
                    "role_breakdown",
                )
            }
            "device_links" => {
                let links = read_vec(reader, |reader| {
                    let mut device_hash = None;
                    let mut opt_in = None;
                    decode_struct(reader, Some(2), |key, reader| match key {
                        "device_hash" => {
                            assign_once(&mut device_hash, reader.read_string()?, "device_hash")
                        }
                        "opt_in" => assign_once(&mut opt_in, reader.read_u64()?, "opt_in"),
                        other => Err(DecodeError::UnknownField(other.to_string())),
                    })?;
                    Ok(ad_market::DeviceLinkOptIn {
                        device_hash: device_hash.ok_or(DecodeError::MissingField("device_hash"))?,
                        opt_in: opt_in.unwrap_or(0) != 0,
                    })
                })?;
                assign_once(&mut device_links, links, "device_links")
            }
            "provider_signature" => assign_once(
                &mut provider_signature,
                read_bytes_field(reader, "provider_signature")?,
                "provider_signature",
            ),
            "publisher_signature" => assign_once(
                &mut publisher_signature,
                read_bytes_field(reader, "publisher_signature")?,
                "publisher_signature",
            ),
            "signature_nonce" => {
                assign_once(&mut signature_nonce, reader.read_u64()?, "signature_nonce")
            }
            other => Err(DecodeError::UnknownField(other.to_string())),
        })?;

        let receipt_type = receipt_type.ok_or(DecodeError::MissingField("type"))?;
        match receipt_type.as_str() {
            "storage" => Ok(Receipt::Storage(StorageReceipt {
                contract_id: contract_id.ok_or(DecodeError::MissingField("contract_id"))?,
                provider: provider.ok_or(DecodeError::MissingField("provider"))?,
                bytes: bytes.ok_or(DecodeError::MissingField("bytes"))?,
                price: price.ok_or(DecodeError::MissingField("price"))?,
                block_height: block_height.ok_or(DecodeError::MissingField("block_height"))?,
                provider_escrow: provider_escrow
                    .ok_or(DecodeError::MissingField("provider_escrow"))?,
                provider_signature: provider_signature
                    .ok_or(DecodeError::MissingField("provider_signature"))?,
                signature_nonce: signature_nonce
                    .ok_or(DecodeError::MissingField("signature_nonce"))?,
            })),
            "compute" => Ok(Receipt::Compute(ComputeReceipt {
                job_id: job_id.ok_or(DecodeError::MissingField("job_id"))?,
                provider: provider.ok_or(DecodeError::MissingField("provider"))?,
                compute_units: compute_units.ok_or(DecodeError::MissingField("compute_units"))?,
                payment: payment.ok_or(DecodeError::MissingField("payment"))?,
                block_height: block_height.ok_or(DecodeError::MissingField("block_height"))?,
                verified: verified.ok_or(DecodeError::MissingField("verified"))? != 0,
                provider_signature: provider_signature
                    .ok_or(DecodeError::MissingField("provider_signature"))?,
                signature_nonce: signature_nonce
                    .ok_or(DecodeError::MissingField("signature_nonce"))?,
            })),
            "energy" => Ok(Receipt::Energy(EnergyReceipt {
                contract_id: contract_id.ok_or(DecodeError::MissingField("contract_id"))?,
                provider: provider.ok_or(DecodeError::MissingField("provider"))?,
                energy_units: energy_units.ok_or(DecodeError::MissingField("energy_units"))?,
                price: price.ok_or(DecodeError::MissingField("price"))?,
                block_height: block_height.ok_or(DecodeError::MissingField("block_height"))?,
                proof_hash: proof_hash.ok_or(DecodeError::MissingField("proof_hash"))?,
                provider_signature: provider_signature
                    .ok_or(DecodeError::MissingField("provider_signature"))?,
                signature_nonce: signature_nonce
                    .ok_or(DecodeError::MissingField("signature_nonce"))?,
            })),
            "ad" => Ok(Receipt::Ad(AdReceipt {
                campaign_id: campaign_id.ok_or(DecodeError::MissingField("campaign_id"))?,
                creative_id: creative_id.unwrap_or_default(),
                publisher: publisher.ok_or(DecodeError::MissingField("publisher"))?,
                impressions: impressions.ok_or(DecodeError::MissingField("impressions"))?,
                spend: spend.ok_or(DecodeError::MissingField("spend"))?,
                block_height: block_height.ok_or(DecodeError::MissingField("block_height"))?,
                conversions: conversions.ok_or(DecodeError::MissingField("conversions"))? as u32,
                claim_routes: claim_routes.unwrap_or_default(),
                role_breakdown,
                device_links: device_links.unwrap_or_default(),
                publisher_signature: publisher_signature
                    .ok_or(DecodeError::MissingField("publisher_signature"))?,
                signature_nonce: signature_nonce
                    .ok_or(DecodeError::MissingField("signature_nonce"))?,
            })),
            _ => Err(DecodeError::InvalidFieldValue {
                field: "type",
                reason: format!("unknown receipt type: {}", receipt_type),
            }),
        }
    })
}

pub(crate) fn write_optional_receipt_header(
    writer: &mut Writer,
    header: &Option<ReceiptHeader>,
) -> EncodeResult<()> {
    match header {
        Some(h) => {
            writer.write_u64(1);
            write_receipt_header(writer, h)
        }
        None => {
            writer.write_u64(0);
            Ok(())
        }
    }
}

pub(crate) fn read_optional_receipt_header(
    reader: &mut Reader<'_>,
) -> Result<Option<ReceiptHeader>, DecodeError> {
    let flag = reader.read_u64()?;
    if flag == 0 {
        Ok(None)
    } else {
        read_receipt_header(reader).map(Some)
    }
}

pub(crate) fn write_receipt_header(
    writer: &mut Writer,
    header: &ReceiptHeader,
) -> EncodeResult<()> {
    let mut result: EncodeResult<()> = Ok(());
    writer.write_struct(|struct_writer| {
        struct_writer.field_u64("shard_count", header.shard_count as u64);
        struct_writer.field_with("shard_roots", |w| {
            if result.is_ok() {
                if let Err(err) = write_root_vec(w, &header.shard_roots) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_with("blob_commitments", |w| {
            if result.is_ok() {
                if let Err(err) = write_root_vec(w, &header.blob_commitments) {
                    result = Err(err);
                }
            }
        });
        struct_writer.field_u64("available_until", header.available_until);
        struct_writer.field_u64(
            "aggregate_scheme",
            match header.aggregate_scheme {
                ReceiptAggregateScheme::None => 0,
                ReceiptAggregateScheme::BatchEd25519 => 1,
            },
        );
        struct_writer.field_with("aggregate_sig", |w| {
            write_fixed(w, &header.aggregate_sig);
        });
    });
    result
}

pub(crate) fn read_receipt_header(reader: &mut Reader<'_>) -> Result<ReceiptHeader, DecodeError> {
    let mut shard_count = None;
    let mut shard_roots = None;
    let mut blob_commitments = None;
    let mut available_until = None;
    let mut aggregate_scheme = None;
    let mut aggregate_sig = None;

    decode_struct(reader, None, |key, reader| match key {
        "shard_count" => assign_once(&mut shard_count, reader.read_u64()?, "shard_count"),
        "shard_roots" => assign_once(&mut shard_roots, read_root_vec(reader)?, "shard_roots"),
        "blob_commitments" => assign_once(
            &mut blob_commitments,
            read_root_vec(reader)?,
            "blob_commitments",
        ),
        "available_until" => {
            assign_once(&mut available_until, reader.read_u64()?, "available_until")
        }
        "aggregate_scheme" => assign_once(
            &mut aggregate_scheme,
            reader.read_u64()?,
            "aggregate_scheme",
        ),
        "aggregate_sig" => assign_once(&mut aggregate_sig, read_fixed(reader)?, "aggregate_sig"),
        other => Err(DecodeError::UnknownField(other.to_string())),
    })?;

    let shard_count_u16: u16 =
        shard_count
            .unwrap_or_default()
            .try_into()
            .map_err(|_| DecodeError::InvalidFieldValue {
                field: "shard_count",
                reason: "value does not fit u16".into(),
            })?;
    let scheme = match aggregate_scheme.unwrap_or_default() {
        0 => ReceiptAggregateScheme::None,
        1 => ReceiptAggregateScheme::BatchEd25519,
        other => {
            return Err(DecodeError::InvalidFieldValue {
                field: "aggregate_scheme",
                reason: format!("unknown value {other}"),
            })
        }
    };

    Ok(ReceiptHeader {
        shard_count: shard_count_u16,
        shard_roots: shard_roots.unwrap_or_default(),
        blob_commitments: blob_commitments.unwrap_or_default(),
        available_until: available_until.unwrap_or_default(),
        aggregate_scheme: scheme,
        aggregate_sig: aggregate_sig.unwrap_or([0; 32]),
    })
}

fn write_root_vec(writer: &mut Writer, roots: &[[u8; 32]]) -> EncodeResult<()> {
    write_vec(writer, roots, "l2_roots", |writer, root| {
        write_fixed(writer, root);
        Ok(())
    })
}

fn read_root_vec(reader: &mut Reader<'_>) -> Result<Vec<[u8; 32]>, DecodeError> {
    read_vec(reader, |reader| read_fixed(reader))
}

fn write_u32_vec(writer: &mut Writer, values: &[u32]) -> EncodeResult<()> {
    write_vec(writer, values, "l2_sizes", |writer, value| {
        writer.write_u32(*value);
        Ok(())
    })
}

fn read_u32_vec(reader: &mut Reader<'_>) -> Result<Vec<u32>, DecodeError> {
    read_vec(reader, |reader| {
        reader.read_u32().map_err(DecodeError::from)
    })
}

fn write_bytes(writer: &mut Writer, value: &[u8], field: &'static str) -> EncodeResult<()> {
    let _ = u64::try_from(value.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_bytes(value);
    Ok(())
}

fn read_bytes_field(reader: &mut Reader<'_>, field: &'static str) -> Result<Vec<u8>, DecodeError> {
    let len = reader.read_u64()?;
    let len_usize = usize::try_from(len).map_err(|_| DecodeError::InvalidFieldValue {
        field,
        reason: format!("length overflow {}", len),
    })?;
    reader
        .read_exact(len_usize)
        .map(|bytes| bytes.to_vec())
        .map_err(DecodeError::from)
}

fn write_fixed(writer: &mut Writer, value: &[u8; 32]) {
    writer.write_u64(32);
    for byte in value {
        writer.write_u8(*byte);
    }
}

fn read_fixed(reader: &mut Reader<'_>) -> Result<[u8; 32], DecodeError> {
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

fn read_vec<T, F>(reader: &mut Reader<'_>, mut read: F) -> Result<Vec<T>, DecodeError>
where
    F: FnMut(&mut Reader<'_>) -> Result<T, DecodeError>,
{
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(read(reader)?);
    }
    Ok(values)
}

fn read_retune_hint(reader: &mut Reader<'_>) -> Result<i8, DecodeError> {
    let value = reader.read_i64()?;
    i8::try_from(value).map_err(|_| DecodeError::InvalidFieldValue {
        field: "retune_hint",
        reason: format!("expected i8 got {value}"),
    })
}

/// Encode receipts to bytes for block hashing (consensus-critical)
///
/// This function serializes receipts into the canonical binary format
/// for inclusion in block hash calculation. The serialized bytes ensure
/// deterministic hashing across all nodes.
///
/// # Arguments
/// * `receipts` - Slice of receipts to serialize
///
/// # Returns
/// * `Ok(Vec<u8>)` - Serialized receipt bytes
/// * `Err(EncodeError)` - If serialization fails
///
/// # Example
/// ```ignore
/// let receipts = vec![Receipt::Ad(ad_receipt)];
/// let bytes = encode_receipts(&receipts)?;
/// // Use bytes in BlockEncoder for hashing
/// ```
pub fn encode_receipts(receipts: &[Receipt]) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::with_capacity(receipts.len() * 256); // Estimate 256 bytes per receipt
    write_receipts(&mut writer, receipts)?;
    Ok(writer.finish())
}

pub fn decode_receipts(bytes: &[u8]) -> binary_struct::Result<Vec<Receipt>> {
    let mut reader = Reader::new(bytes);
    let receipts = read_receipts(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(receipts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{FeeLane, RawTxPayload, SignedTransaction, TxSignature, TxVersion};

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
            index: 4,
            previous_hash: "prev".into(),
            timestamp_millis: 99,
            transactions: vec![sample_tx()],
            difficulty: 5,
            retune_hint: -2,
            nonce: 42,
            hash: "hash".into(),
            coinbase_block: TokenAmount::new(10),
            coinbase_industrial: TokenAmount::new(11),
            storage_sub: TokenAmount::new(12),
            read_sub: TokenAmount::new(13),
            read_sub_viewer: TokenAmount::new(2),
            read_sub_host: TokenAmount::new(3),
            read_sub_hardware: TokenAmount::new(4),
            read_sub_verifier: TokenAmount::new(1),
            read_sub_liquidity: TokenAmount::new(3),
            ad_viewer: TokenAmount::new(6),
            ad_host: TokenAmount::new(7),
            ad_hardware: TokenAmount::new(8),
            ad_verifier: TokenAmount::new(9),
            ad_liquidity: TokenAmount::new(10),
            ad_miner: TokenAmount::new(11),
            treasury_events: Vec::new(),
            ad_total_usd_micros: 123_000,
            ad_settlement_count: 2,
            ad_oracle_price_usd_micros: 456,
            compute_sub: TokenAmount::new(14),
            proof_rebate: TokenAmount::new(15),
            read_root: [1u8; 32],
            fee_checksum: "fee".into(),
            state_root: "state".into(),
            base_fee: 7,
            l2_roots: vec![[2u8; 32], [3u8; 32]],
            l2_sizes: vec![4, 5],
            vdf_commit: [4u8; 32],
            vdf_output: [5u8; 32],
            vdf_proof: vec![1, 2, 3],
            #[cfg(feature = "quantum")]
            dilithium_pubkey: vec![1, 3, 5],
            #[cfg(feature = "quantum")]
            dilithium_sig: vec![2, 4, 6],
            receipts: Vec::new(),
            receipt_header: None,
        }
    }

    #[test]
    fn block_round_trip() {
        let block = sample_block();
        let encoded = encode_block(&block).expect("encode block");
        let decoded = decode_block(&encoded).expect("decode block");
        assert_eq!(decoded.index, block.index);
        assert_eq!(decoded.previous_hash, block.previous_hash);
        assert_eq!(decoded.timestamp_millis, block.timestamp_millis);
        assert_eq!(decoded.transactions.len(), block.transactions.len());
        assert_eq!(decoded.difficulty, block.difficulty);
        assert_eq!(decoded.retune_hint, block.retune_hint);
        assert_eq!(decoded.nonce, block.nonce);
        assert_eq!(decoded.hash, block.hash);
        assert_eq!(decoded.coinbase_block.get(), block.coinbase_block.get());
        assert_eq!(
            decoded.coinbase_industrial.get(),
            block.coinbase_industrial.get()
        );
        assert_eq!(decoded.storage_sub.get(), block.storage_sub.get());
        assert_eq!(decoded.read_sub.get(), block.read_sub.get());
        assert_eq!(decoded.compute_sub.get(), block.compute_sub.get());
        assert_eq!(decoded.proof_rebate.get(), block.proof_rebate.get());
        assert_eq!(decoded.read_root, block.read_root);
        assert_eq!(decoded.fee_checksum, block.fee_checksum);
        assert_eq!(decoded.state_root, block.state_root);
        assert_eq!(decoded.base_fee, block.base_fee);
        assert_eq!(decoded.l2_roots, block.l2_roots);
        assert_eq!(decoded.l2_sizes, block.l2_sizes);
        assert_eq!(decoded.vdf_commit, block.vdf_commit);
        assert_eq!(decoded.vdf_output, block.vdf_output);
        assert_eq!(decoded.vdf_proof, block.vdf_proof);
        #[cfg(feature = "quantum")]
        {
            assert_eq!(decoded.dilithium_pubkey, block.dilithium_pubkey);
            assert_eq!(decoded.dilithium_sig, block.dilithium_sig);
        }
    }
}
