#![forbid(unsafe_code)]

use std::collections::{BTreeMap, VecDeque};
use std::convert::TryFrom;
use std::fmt;

use dex::amm::Pool;
use dex::escrow::{Escrow, EscrowEntry, EscrowId, EscrowSnapshot, HashAlgo, PaymentProof};
use foundation_serialization::binary_cursor::{CursorError, Reader, Writer};

use crate::util::binary_struct::{self, assign_once, decode_struct, ensure_exhausted, DecodeError};

use super::order_book::{Order, OrderBook, Side};
use super::storage::EscrowState;

/// Result alias returned by encoding helpers.
pub(crate) type EncodeResult<T> = Result<T, EncodeError>;

/// Error raised when encoding encounters an overflow.
#[derive(Debug)]
pub(crate) enum EncodeError {
    /// Collection length exceeded `u64::MAX`.
    LengthOverflow(&'static str),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::LengthOverflow(field) => {
                write!(f, "{field} length exceeds u64::MAX")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Trade log record persisted to sled.
#[derive(Debug, Clone)]
pub(crate) struct TradeLogRecord {
    pub buy: Order,
    pub sell: Order,
    pub quantity: u64,
    pub proof: PaymentProof,
}

pub(crate) fn encode_order_book(book: &OrderBook) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    write_order_book(&mut writer, book)?;
    Ok(writer.finish())
}

pub(crate) fn decode_order_book(bytes: &[u8]) -> binary_struct::Result<OrderBook> {
    let mut reader = Reader::new(bytes);
    let book = read_order_book(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(book)
}

pub(crate) fn encode_trade_log(record: &TradeLogRecord) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    write_trade_log(&mut writer, record)?;
    Ok(writer.finish())
}

pub(crate) fn decode_trade_log(bytes: &[u8]) -> binary_struct::Result<TradeLogRecord> {
    let mut reader = Reader::new(bytes);
    let record = read_trade_log(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(record)
}

pub(crate) fn encode_escrow_state(state: &EscrowState) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    write_escrow_state(&mut writer, state)?;
    Ok(writer.finish())
}

pub(crate) fn decode_escrow_state(bytes: &[u8]) -> binary_struct::Result<EscrowState> {
    let mut reader = Reader::new(bytes);
    let state = read_escrow_state(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(state)
}

pub(crate) fn encode_pool(pool: &Pool) -> EncodeResult<Vec<u8>> {
    let mut writer = Writer::new();
    write_pool(&mut writer, pool);
    Ok(writer.finish())
}

pub(crate) fn decode_pool(bytes: &[u8]) -> binary_struct::Result<Pool> {
    let mut reader = Reader::new(bytes);
    let pool = read_pool(&mut reader)?;
    ensure_exhausted(&reader)?;
    Ok(pool)
}

fn write_order_book(writer: &mut Writer, book: &OrderBook) -> EncodeResult<()> {
    writer.write_u64(3);
    writer.write_string("bids");
    write_order_levels(writer, &book.bids, "bids")?;
    writer.write_string("asks");
    write_order_levels(writer, &book.asks, "asks")?;
    writer.write_string("next_id");
    writer.write_u64(book.next_identifier());
    Ok(())
}

fn write_order_levels(
    writer: &mut Writer,
    levels: &BTreeMap<u64, VecDeque<Order>>,
    field: &'static str,
) -> EncodeResult<()> {
    let len = u64::try_from(levels.len()).map_err(|_| EncodeError::LengthOverflow(field))?;
    writer.write_u64(len);
    for (price, orders) in levels {
        writer.write_u64(*price);
        let order_len =
            u64::try_from(orders.len()).map_err(|_| EncodeError::LengthOverflow("orders"))?;
        writer.write_u64(order_len);
        for order in orders {
            write_order(writer, order);
        }
    }
    Ok(())
}

fn write_order(writer: &mut Writer, order: &Order) {
    writer.write_u64(6);
    writer.write_string("id");
    writer.write_u64(order.id);
    writer.write_string("account");
    writer.write_string(&order.account);
    writer.write_string("side");
    writer.write_u32(match order.side {
        Side::Buy => 0,
        Side::Sell => 1,
    });
    writer.write_string("amount");
    writer.write_u64(order.amount);
    writer.write_string("price");
    writer.write_u64(order.price);
    writer.write_string("max_slippage_bps");
    writer.write_u64(order.max_slippage_bps);
}

fn read_order_book(reader: &mut Reader<'_>) -> binary_struct::Result<OrderBook> {
    let mut bids = None;
    let mut asks = None;
    let mut next_id = None;
    decode_struct(reader, Some(3), |key, reader| match key {
        "bids" => {
            let value = read_order_levels(reader)?;
            assign_once(&mut bids, value, "bids")
        }
        "asks" => {
            let value = read_order_levels(reader)?;
            assign_once(&mut asks, value, "asks")
        }
        "next_id" => {
            let value = reader.read_u64()?;
            assign_once(&mut next_id, value, "next_id")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    let mut book = OrderBook::default();
    book.bids = bids.ok_or(DecodeError::MissingField("bids"))?;
    book.asks = asks.ok_or(DecodeError::MissingField("asks"))?;
    book.set_next_identifier(next_id.ok_or(DecodeError::MissingField("next_id"))?);
    Ok(book)
}

fn read_order_levels(
    reader: &mut Reader<'_>,
) -> binary_struct::Result<BTreeMap<u64, VecDeque<Order>>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut levels = BTreeMap::new();
    for _ in 0..len {
        let price = reader.read_u64()?;
        let orders_len = reader.read_u64()?;
        let orders_len = usize::try_from(orders_len)
            .map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(orders_len)))?;
        let mut orders = Vec::with_capacity(orders_len);
        for _ in 0..orders_len {
            orders.push(read_order(reader)?);
        }
        levels.insert(price, VecDeque::from(orders));
    }
    Ok(levels)
}

fn read_order(reader: &mut Reader<'_>) -> binary_struct::Result<Order> {
    let mut id = None;
    let mut account = None;
    let mut side = None;
    let mut amount = None;
    let mut price = None;
    let mut max_slippage_bps = None;
    decode_struct(reader, Some(6), |key, reader| match key {
        "id" => {
            let value = reader.read_u64()?;
            assign_once(&mut id, value, "id")
        }
        "account" => {
            let value = reader.read_string()?;
            assign_once(&mut account, value, "account")
        }
        "side" => {
            let raw = reader.read_u32()?;
            let value = match raw {
                0 => Side::Buy,
                1 => Side::Sell,
                other => {
                    return Err(DecodeError::InvalidEnumDiscriminant {
                        ty: "Side",
                        value: other,
                    })
                }
            };
            assign_once(&mut side, value, "side")
        }
        "amount" => {
            let value = reader.read_u64()?;
            assign_once(&mut amount, value, "amount")
        }
        "price" => {
            let value = reader.read_u64()?;
            assign_once(&mut price, value, "price")
        }
        "max_slippage_bps" => {
            let value = reader.read_u64()?;
            assign_once(&mut max_slippage_bps, value, "max_slippage_bps")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(Order {
        id: id.ok_or(DecodeError::MissingField("id"))?,
        account: account.ok_or(DecodeError::MissingField("account"))?,
        side: side.ok_or(DecodeError::MissingField("side"))?,
        amount: amount.ok_or(DecodeError::MissingField("amount"))?,
        price: price.ok_or(DecodeError::MissingField("price"))?,
        max_slippage_bps: max_slippage_bps.ok_or(DecodeError::MissingField("max_slippage_bps"))?,
    })
}

fn write_trade_log(writer: &mut Writer, record: &TradeLogRecord) -> EncodeResult<()> {
    writer.write_u64(4);
    write_order(writer, &record.buy);
    write_order(writer, &record.sell);
    writer.write_u64(record.quantity);
    write_payment_proof(writer, &record.proof)?;
    Ok(())
}

fn read_trade_log(reader: &mut Reader<'_>) -> binary_struct::Result<TradeLogRecord> {
    let field_count = reader.read_u64()?;
    if field_count != 4 {
        return Err(DecodeError::InvalidFieldCount {
            expected: 4,
            actual: field_count,
        });
    }
    let buy = read_order(reader)?;
    let sell = read_order(reader)?;
    let quantity = reader.read_u64()?;
    let proof = read_payment_proof(reader)?;
    Ok(TradeLogRecord {
        buy,
        sell,
        quantity,
        proof,
    })
}

fn write_escrow_state(writer: &mut Writer, state: &EscrowState) -> EncodeResult<()> {
    writer.write_u64(2);
    writer.write_string("escrow");
    write_escrow(writer, &state.escrow)?;
    writer.write_string("locks");
    write_locks(writer, &state.locks)?;
    Ok(())
}

fn read_escrow_state(reader: &mut Reader<'_>) -> binary_struct::Result<EscrowState> {
    let mut escrow = None;
    let mut locks = None;
    decode_struct(reader, Some(2), |key, reader| match key {
        "escrow" => {
            let value = read_escrow(reader)?;
            assign_once(&mut escrow, value, "escrow")
        }
        "locks" => {
            let value = read_locks(reader)?;
            assign_once(&mut locks, value, "locks")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(EscrowState {
        escrow: escrow.ok_or(DecodeError::MissingField("escrow"))?,
        locks: locks.ok_or(DecodeError::MissingField("locks"))?,
    })
}

fn write_escrow(writer: &mut Writer, escrow: &Escrow) -> EncodeResult<()> {
    let snapshot = escrow.snapshot();
    writer.write_u64(3);
    writer.write_string("entries");
    write_escrow_entries(writer, &snapshot.entries)?;
    writer.write_string("next_id");
    writer.write_u64(snapshot.next_id);
    writer.write_string("total_locked");
    writer.write_u64(snapshot.total_locked);
    Ok(())
}

fn write_escrow_entries(
    writer: &mut Writer,
    entries: &[(EscrowId, EscrowEntry)],
) -> EncodeResult<()> {
    let len = u64::try_from(entries.len()).map_err(|_| EncodeError::LengthOverflow("entries"))?;
    writer.write_u64(len);
    for (id, entry) in entries {
        writer.write_u64(*id);
        write_escrow_entry(writer, entry)?;
    }
    Ok(())
}

fn write_escrow_entry(writer: &mut Writer, entry: &EscrowEntry) -> EncodeResult<()> {
    writer.write_u64(7);
    writer.write_string("from");
    writer.write_string(&entry.from);
    writer.write_string("to");
    writer.write_string(&entry.to);
    writer.write_string("total");
    writer.write_u64(entry.total);
    writer.write_string("released");
    writer.write_u64(entry.released);
    writer.write_string("payments");
    write_u64_vec(writer, &entry.payments)?;
    writer.write_string("root");
    write_fixed_u8_array(writer, &entry.root);
    writer.write_string("algo");
    write_hash_algo(writer, entry.algo);
    Ok(())
}

fn write_u64_vec(writer: &mut Writer, values: &[u64]) -> EncodeResult<()> {
    let len = u64::try_from(values.len()).map_err(|_| EncodeError::LengthOverflow("payments"))?;
    writer.write_u64(len);
    for value in values {
        writer.write_u64(*value);
    }
    Ok(())
}

fn write_fixed_u8_array(writer: &mut Writer, value: &[u8; 32]) {
    writer.write_u64(32);
    for byte in value {
        writer.write_u8(*byte);
    }
}

fn write_hash_algo(writer: &mut Writer, algo: HashAlgo) {
    let variant = match algo {
        HashAlgo::Blake3 => 0,
        HashAlgo::Sha3 => 1,
    };
    writer.write_u32(variant);
}

fn write_payment_proof(writer: &mut Writer, proof: &PaymentProof) -> EncodeResult<()> {
    writer.write_u64(3);
    writer.write_string("leaf");
    write_fixed_u8_array(writer, &proof.leaf);
    writer.write_string("path");
    write_hash_path(writer, &proof.path)?;
    writer.write_string("algo");
    write_hash_algo(writer, proof.algo);
    Ok(())
}

fn write_hash_path(writer: &mut Writer, path: &[[u8; 32]]) -> EncodeResult<()> {
    let len = u64::try_from(path.len()).map_err(|_| EncodeError::LengthOverflow("path"))?;
    writer.write_u64(len);
    for entry in path {
        write_fixed_u8_array(writer, entry);
    }
    Ok(())
}

fn write_locks(
    writer: &mut Writer,
    locks: &BTreeMap<EscrowId, (Order, Order, u64, u64)>,
) -> EncodeResult<()> {
    let len = u64::try_from(locks.len()).map_err(|_| EncodeError::LengthOverflow("locks"))?;
    writer.write_u64(len);
    for (id, (buy, sell, qty, locked_at)) in locks {
        writer.write_u64(*id);
        writer.write_u64(4);
        write_order(writer, buy);
        write_order(writer, sell);
        writer.write_u64(*qty);
        writer.write_u64(*locked_at);
    }
    Ok(())
}

fn read_escrow(reader: &mut Reader<'_>) -> binary_struct::Result<Escrow> {
    let mut entries = None;
    let mut next_id = None;
    let mut total_locked = None;
    decode_struct(reader, Some(3), |key, reader| match key {
        "entries" => {
            let value = read_escrow_entries(reader)?;
            assign_once(&mut entries, value, "entries")
        }
        "next_id" => {
            let value = reader.read_u64()?;
            assign_once(&mut next_id, value, "next_id")
        }
        "total_locked" => {
            let value = reader.read_u64()?;
            assign_once(&mut total_locked, value, "total_locked")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    let snapshot = EscrowSnapshot {
        entries: entries.ok_or(DecodeError::MissingField("entries"))?,
        next_id: next_id.ok_or(DecodeError::MissingField("next_id"))?,
        total_locked: total_locked.ok_or(DecodeError::MissingField("total_locked"))?,
    };
    Ok(Escrow::from_snapshot(snapshot))
}

fn read_escrow_entries(
    reader: &mut Reader<'_>,
) -> binary_struct::Result<Vec<(EscrowId, EscrowEntry)>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut entries = Vec::with_capacity(len);
    for _ in 0..len {
        let id = reader.read_u64()?;
        let entry = read_escrow_entry(reader)?;
        entries.push((id, entry));
    }
    Ok(entries)
}

fn read_escrow_entry(reader: &mut Reader<'_>) -> binary_struct::Result<EscrowEntry> {
    let mut from = None;
    let mut to = None;
    let mut total = None;
    let mut released = None;
    let mut payments = None;
    let mut root = None;
    let mut algo = None;
    decode_struct(reader, Some(7), |key, reader| match key {
        "from" => {
            let value = reader.read_string()?;
            assign_once(&mut from, value, "from")
        }
        "to" => {
            let value = reader.read_string()?;
            assign_once(&mut to, value, "to")
        }
        "total" => {
            let value = reader.read_u64()?;
            assign_once(&mut total, value, "total")
        }
        "released" => {
            let value = reader.read_u64()?;
            assign_once(&mut released, value, "released")
        }
        "payments" => {
            let value = read_u64_vec(reader)?;
            assign_once(&mut payments, value, "payments")
        }
        "root" => {
            let value = read_fixed_u8_array(reader, 32)?;
            assign_once(&mut root, value, "root")
        }
        "algo" => {
            let value = read_hash_algo(reader)?;
            assign_once(&mut algo, value, "algo")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(EscrowEntry {
        from: from.ok_or(DecodeError::MissingField("from"))?,
        to: to.ok_or(DecodeError::MissingField("to"))?,
        total: total.ok_or(DecodeError::MissingField("total"))?,
        released: released.ok_or(DecodeError::MissingField("released"))?,
        payments: payments.ok_or(DecodeError::MissingField("payments"))?,
        root: root.ok_or(DecodeError::MissingField("root"))?,
        algo: algo.ok_or(DecodeError::MissingField("algo"))?,
    })
}

fn read_u64_vec(reader: &mut Reader<'_>) -> binary_struct::Result<Vec<u64>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(reader.read_u64()?);
    }
    Ok(values)
}

fn read_fixed_u8_array(
    reader: &mut Reader<'_>,
    expected: usize,
) -> binary_struct::Result<[u8; 32]> {
    let len = reader.read_u64()?;
    if len != expected as u64 {
        return Err(DecodeError::InvalidFieldCount {
            expected: expected as u64,
            actual: len,
        });
    }
    let mut bytes = [0u8; 32];
    for i in 0..expected {
        bytes[i] = reader.read_u8()?;
    }
    Ok(bytes)
}

fn read_hash_algo(reader: &mut Reader<'_>) -> binary_struct::Result<HashAlgo> {
    let raw = reader.read_u32()?;
    match raw {
        0 => Ok(HashAlgo::Blake3),
        1 => Ok(HashAlgo::Sha3),
        other => Err(DecodeError::InvalidEnumDiscriminant {
            ty: "HashAlgo",
            value: other,
        }),
    }
}

fn read_payment_proof(reader: &mut Reader<'_>) -> binary_struct::Result<PaymentProof> {
    let mut leaf = None;
    let mut path = None;
    let mut algo = None;
    decode_struct(reader, Some(3), |key, reader| match key {
        "leaf" => {
            let value = read_fixed_u8_array(reader, 32)?;
            assign_once(&mut leaf, value, "leaf")
        }
        "path" => {
            let value = read_hash_path(reader)?;
            assign_once(&mut path, value, "path")
        }
        "algo" => {
            let value = read_hash_algo(reader)?;
            assign_once(&mut algo, value, "algo")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(PaymentProof {
        leaf: leaf.ok_or(DecodeError::MissingField("leaf"))?,
        path: path.ok_or(DecodeError::MissingField("path"))?,
        algo: algo.ok_or(DecodeError::MissingField("algo"))?,
    })
}

fn read_hash_path(reader: &mut Reader<'_>) -> binary_struct::Result<Vec<[u8; 32]>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut path = Vec::with_capacity(len);
    for _ in 0..len {
        path.push(read_fixed_u8_array(reader, 32)?);
    }
    Ok(path)
}

fn read_locks(
    reader: &mut Reader<'_>,
) -> binary_struct::Result<BTreeMap<EscrowId, (Order, Order, u64, u64)>> {
    let len = reader.read_u64()?;
    let len =
        usize::try_from(len).map_err(|_| DecodeError::Cursor(CursorError::LengthOverflow(len)))?;
    let mut locks = BTreeMap::new();
    for _ in 0..len {
        let id = reader.read_u64()?;
        let tuple_len = reader.read_u64()?;
        if tuple_len != 4 {
            return Err(DecodeError::InvalidFieldCount {
                expected: 4,
                actual: tuple_len,
            });
        }
        let buy = read_order(reader)?;
        let sell = read_order(reader)?;
        let qty = reader.read_u64()?;
        let locked_at = reader.read_u64()?;
        locks.insert(id, (buy, sell, qty, locked_at));
    }
    Ok(locks)
}

fn write_pool(writer: &mut Writer, pool: &Pool) {
    writer.write_u64(3);
    writer.write_string("ct_reserve");
    writer.write_u128(pool.ct_reserve);
    writer.write_string("it_reserve");
    writer.write_u128(pool.it_reserve);
    writer.write_string("total_shares");
    writer.write_u128(pool.total_shares);
}

fn read_pool(reader: &mut Reader<'_>) -> binary_struct::Result<Pool> {
    let mut ct_reserve = None;
    let mut it_reserve = None;
    let mut total_shares = None;
    decode_struct(reader, Some(3), |key, reader| match key {
        "ct_reserve" => {
            let value = reader.read_u128()?;
            assign_once(&mut ct_reserve, value, "ct_reserve")
        }
        "it_reserve" => {
            let value = reader.read_u128()?;
            assign_once(&mut it_reserve, value, "it_reserve")
        }
        "total_shares" => {
            let value = reader.read_u128()?;
            assign_once(&mut total_shares, value, "total_shares")
        }
        other => Err(binary_struct::DecodeError::UnknownField(other.to_owned())),
    })?;
    Ok(Pool {
        ct_reserve: ct_reserve.ok_or(DecodeError::MissingField("ct_reserve"))?,
        it_reserve: it_reserve.ok_or(DecodeError::MissingField("it_reserve"))?,
        total_shares: total_shares.ok_or(DecodeError::MissingField("total_shares"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::binary_codec;
    use foundation_serialization::Serialize;
    use std::collections::VecDeque;

    #[test]
    fn order_book_matches_legacy() {
        let mut book = OrderBook::default();
        let mut bids = VecDeque::new();
        bids.push_back(Order {
            id: 1,
            account: "alice".into(),
            side: Side::Buy,
            amount: 50,
            price: 10,
            max_slippage_bps: 100,
        });
        book.bids.insert(10, bids);
        let mut asks = VecDeque::new();
        asks.push_back(Order {
            id: 2,
            account: "bob".into(),
            side: Side::Sell,
            amount: 40,
            price: 11,
            max_slippage_bps: 50,
        });
        book.asks.insert(11, asks);
        book.set_next_identifier(3);

        let legacy = binary_codec::serialize(&book).expect("legacy encode");
        let manual = encode_order_book(&book).expect("manual encode");
        assert_eq!(legacy, manual);

        let decoded = decode_order_book(&legacy).expect("decode");
        let reencoded = encode_order_book(&decoded).expect("re-encode");
        assert_eq!(legacy, reencoded);
    }

    #[test]
    fn trade_log_matches_legacy() {
        let record = TradeLogRecord {
            buy: Order {
                id: 1,
                account: "alice".into(),
                side: Side::Buy,
                amount: 25,
                price: 9,
                max_slippage_bps: 75,
            },
            sell: Order {
                id: 2,
                account: "bob".into(),
                side: Side::Sell,
                amount: 25,
                price: 9,
                max_slippage_bps: 60,
            },
            quantity: 25,
            proof: PaymentProof {
                leaf: [1u8; 32],
                path: vec![[2u8; 32], [3u8; 32]],
                algo: HashAlgo::Sha3,
            },
        };

        #[derive(Serialize)]
        struct LegacyTradeLog<'a>(&'a Order, &'a Order, u64, &'a PaymentProof);

        let legacy = binary_codec::serialize(&LegacyTradeLog(
            &record.buy,
            &record.sell,
            record.quantity,
            &record.proof,
        ))
        .expect("legacy encode");
        let manual = encode_trade_log(&record).expect("manual encode");
        assert_eq!(legacy, manual);

        let decoded = decode_trade_log(&legacy).expect("decode");
        let reencoded = encode_trade_log(&decoded).expect("re-encode");
        assert_eq!(legacy, reencoded);
    }

    #[test]
    fn escrow_state_matches_legacy() {
        let mut escrow = Escrow::default();
        let first = escrow.lock("alice".into(), "bob".into(), 50);
        escrow.release(first, 20).unwrap();
        let second = escrow.lock_with_algo("carol".into(), "dave".into(), 75, HashAlgo::Sha3);
        let proof = escrow.release(second, 30).unwrap();

        let mut locks = BTreeMap::new();
        locks.insert(
            first,
            (
                Order {
                    id: 10,
                    account: "alice".into(),
                    side: Side::Buy,
                    amount: 50,
                    price: 12,
                    max_slippage_bps: 125,
                },
                Order {
                    id: 11,
                    account: "bob".into(),
                    side: Side::Sell,
                    amount: 50,
                    price: 12,
                    max_slippage_bps: 110,
                },
                50,
                1_700_000,
            ),
        );
        locks.insert(
            second,
            (
                Order {
                    id: 12,
                    account: "carol".into(),
                    side: Side::Buy,
                    amount: 75,
                    price: 15,
                    max_slippage_bps: 95,
                },
                Order {
                    id: 13,
                    account: "dave".into(),
                    side: Side::Sell,
                    amount: 75,
                    price: 15,
                    max_slippage_bps: 90,
                },
                75,
                1_800_000,
            ),
        );

        let state = EscrowState { escrow, locks };

        #[derive(Serialize)]
        struct LegacyEscrowState<'a> {
            escrow: &'a Escrow,
            locks: &'a BTreeMap<EscrowId, (Order, Order, u64, u64)>,
        }

        let legacy = binary_codec::serialize(&LegacyEscrowState {
            escrow: &state.escrow,
            locks: &state.locks,
        })
        .expect("legacy encode");
        let manual = encode_escrow_state(&state).expect("manual encode");
        assert_eq!(legacy, manual);

        let decoded = decode_escrow_state(&legacy).expect("decode");
        let reencoded = encode_escrow_state(&decoded).expect("re-encode");
        assert_eq!(legacy, reencoded);

        // Ensure the proof remains valid after roundtrip
        let decoded_proof = decoded.escrow.proof(second, 0).expect("proof");
        assert_eq!(decoded_proof.algo, proof.algo);
    }

    #[test]
    fn pool_matches_legacy() {
        let mut pool = Pool::default();
        pool.ct_reserve = 1_000;
        pool.it_reserve = 2_000;
        pool.total_shares = 500;

        let legacy = binary_codec::serialize(&pool).expect("legacy encode");
        let manual = encode_pool(&pool).expect("manual encode");
        assert_eq!(legacy, manual);

        let decoded = decode_pool(&legacy).expect("decode");
        let reencoded = encode_pool(&decoded).expect("re-encode");
        assert_eq!(legacy, reencoded);
    }
}
