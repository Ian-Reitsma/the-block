import pytest
import the_block


def make_bc(tmp_path):
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    return bc


def test_unknown_sender(tmp_path):
    bc = make_bc(tmp_path / "unk")
    priv, _ = the_block.generate_keypair()
    payload = the_block.RawTxPayload(
        from_="ghost",
        to="alice",
        amount_consumer=1,
        amount_industrial=0,
        fee=0,
        fee_selector=0,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    with pytest.raises(the_block.ErrUnknownSender):
        bc.submit_transaction(stx)


def test_bad_nonce(tmp_path):
    bc = make_bc(tmp_path / "nonce")
    bc.add_account("miner", 10, 0)
    bc.add_account("alice", 0, 0)
    priv, _ = the_block.generate_keypair()
    payload = the_block.RawTxPayload(
        from_="miner",
        to="alice",
        amount_consumer=1,
        amount_industrial=0,
        fee=0,
        fee_selector=0,
        nonce=2,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    with pytest.raises(the_block.ErrBadNonce):
        bc.submit_transaction(stx)


def test_insufficient_balance(tmp_path):
    bc = make_bc(tmp_path / "bal")
    bc.add_account("miner", 1, 0)
    bc.add_account("alice", 0, 0)
    priv, _ = the_block.generate_keypair()
    payload = the_block.RawTxPayload(
        from_="miner",
        to="alice",
        amount_consumer=10,
        amount_industrial=0,
        fee=0,
        fee_selector=0,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    with pytest.raises(the_block.ErrInsufficientBalance):
        bc.submit_transaction(stx)
