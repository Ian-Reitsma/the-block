import pytest
import the_block


def make_chain(tmp_path):
    path = tmp_path / "code_chain"
    bc = the_block.Blockchain.with_difficulty(str(path), 1)
    bc.genesis_block()
    bc.min_fee_per_byte_consumer = 0
    bc.min_fee_per_byte_industrial = 0
    return bc


def trigger_unknown_sender(tmp_path):
    bc = make_chain(tmp_path)
    bc.add_account("alice", 0, 0)
    priv, _ = the_block.generate_keypair()
    payload = the_block.RawTxPayload(
        from_="ghost",
        to="alice",
        amount_consumer=1,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_insufficient_balance(tmp_path):
    bc = make_chain(tmp_path)
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 0, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=1,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_nonce_gap(tmp_path):
    bc = make_chain(tmp_path)
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=2,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_invalid_selector(tmp_path):
    bc = make_chain(tmp_path)
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=255,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_bad_signature(tmp_path):
    bc = make_chain(tmp_path)
    priv, pub = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    stx.public_key = bytes([stx.public_key[0] ^ 0xFF]) + stx.public_key[1:]
    bc.submit_transaction(stx)


def trigger_duplicate(tmp_path):
    bc = make_chain(tmp_path)
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)
    bc.submit_transaction(stx)


def trigger_not_found(tmp_path):
    bc = make_chain(tmp_path)
    bc.drop_transaction("alice", 1)


def trigger_balance_overflow(tmp_path):
    bc = make_chain(tmp_path)
    bc.add_account("alice", 2**64 - 1, 0)
    priv, _ = the_block.generate_keypair()
    payload1 = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=1,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    tx1 = the_block.sign_tx(list(priv), payload1)
    bc.submit_transaction(tx1)
    payload2 = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=2**64 - 1,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=2,
        memo=b"",
    )
    tx2 = the_block.sign_tx(list(priv), payload2)
    bc.submit_transaction(tx2)


def trigger_fee_too_large(tmp_path):
    bc = make_chain(tmp_path)
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=1 << 63,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_fee_overflow(tmp_path):
    bc = make_chain(tmp_path)
    priv, _ = the_block.generate_keypair()
    max_u64 = (1 << 64) - 1
    bc.add_account("alice", max_u64, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=max_u64,
        amount_industrial=0,
        fee=1,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_fee_too_low(tmp_path):
    bc = make_chain(tmp_path)
    bc.min_fee_per_byte_consumer = 1
    bc.min_fee_per_byte_industrial = 1
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_mempool_full(tmp_path):
    bc = make_chain(tmp_path)
    bc.max_mempool_size_consumer = 0
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)


def trigger_lock_poisoned(tmp_path):
    bc = make_chain(tmp_path)
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    the_block.poison_mempool(bc)
    bc.submit_transaction(stx)


def trigger_pending_limit(tmp_path):
    bc = make_chain(tmp_path)
    bc.max_pending_per_account = 1
    priv, _ = the_block.generate_keypair()
    bc.add_account("alice", 10, 0)
    payload1 = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=1,
        memo=b"",
    )
    tx1 = the_block.sign_tx(list(priv), payload1)
    bc.submit_transaction(tx1)
    payload2 = the_block.RawTxPayload(
        from_="alice",
        to="alice",
        amount_consumer=0,
        amount_industrial=0,
        fee=0,
        pct=100,
        nonce=2,
        memo=b"",
    )
    tx2 = the_block.sign_tx(list(priv), payload2)
    bc.submit_transaction(tx2)


CASES = [
    (trigger_unknown_sender, the_block.ErrUnknownSender, the_block.ERR_UNKNOWN_SENDER),
    (
        trigger_insufficient_balance,
        the_block.ErrInsufficientBalance,
        the_block.ERR_INSUFFICIENT_BALANCE,
    ),
    (trigger_nonce_gap, the_block.ErrNonceGap, the_block.ERR_NONCE_GAP),
    (
        trigger_invalid_selector,
        the_block.ErrInvalidSelector,
        the_block.ERR_INVALID_SELECTOR,
    ),
    (trigger_bad_signature, the_block.ErrBadSignature, the_block.ERR_BAD_SIGNATURE),
    (trigger_duplicate, the_block.ErrDuplicateTx, the_block.ERR_DUPLICATE),
    (trigger_not_found, the_block.ErrTxNotFound, the_block.ERR_NOT_FOUND),
    (trigger_balance_overflow, ValueError, the_block.ERR_BALANCE_OVERFLOW),
    (trigger_fee_too_large, the_block.ErrFeeTooLarge, the_block.ERR_FEE_TOO_LARGE),
    (trigger_fee_overflow, the_block.ErrFeeOverflow, the_block.ERR_FEE_OVERFLOW),
    (trigger_fee_too_low, the_block.ErrFeeTooLow, the_block.ERR_FEE_TOO_LOW),
    (trigger_mempool_full, the_block.ErrMempoolFull, the_block.ERR_MEMPOOL_FULL),
    (trigger_lock_poisoned, the_block.ErrLockPoisoned, the_block.ERR_LOCK_POISONED),
    (trigger_pending_limit, the_block.ErrPendingLimit, the_block.ERR_PENDING_LIMIT),
]


@pytest.mark.parametrize("trigger,exc,code", CASES)
def test_tx_error_codes(tmp_path, trigger, exc, code):
    with pytest.raises(exc) as err:
        trigger(tmp_path)
    assert err.value.code == code
