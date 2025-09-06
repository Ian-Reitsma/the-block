import os
import time

import pytest
import the_block


def test_invalid_env_raises(tmp_path):
    os.environ["TB_PURGE_LOOP_SECS"] = "abc"
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    with pytest.raises(ValueError) as exc:
        the_block.maybe_spawn_purge_loop(bc, flag)
    assert "TB_PURGE_LOOP_SECS" in str(exc.value)
    del os.environ["TB_PURGE_LOOP_SECS"]


def test_zero_env_raises(tmp_path):
    os.environ["TB_PURGE_LOOP_SECS"] = "0"
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    with pytest.raises(ValueError):
        the_block.maybe_spawn_purge_loop(bc, flag)
    del os.environ["TB_PURGE_LOOP_SECS"]


def test_negative_env_raises(tmp_path, monkeypatch):
    monkeypatch.setenv("TB_PURGE_LOOP_SECS", "-5")
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    with pytest.raises(ValueError) as exc:
        the_block.maybe_spawn_purge_loop(bc, flag)
    assert "TB_PURGE_LOOP_SECS" in str(exc.value)


def test_missing_env_raises(tmp_path):
    if "TB_PURGE_LOOP_SECS" in os.environ:
        del os.environ["TB_PURGE_LOOP_SECS"]
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    with pytest.raises(ValueError):
        the_block.maybe_spawn_purge_loop(bc, flag)


def _parse_metrics(text: str) -> dict[str, int]:
    data: dict[str, int] = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) == 2 and parts[1].isdigit():
            data[parts[0]] = int(parts[1])
    return data


def test_valid_env_returns_handle(tmp_path):
    if not hasattr(the_block, "gather_metrics"):
        pytest.skip("telemetry not enabled")

    os.environ["TB_PURGE_LOOP_SECS"] = "1"
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    bc.add_account("alice", 10_000, 0)
    bc.add_account("bob", 0, 0)
    bc.add_account("carol", 10_000, 0)
    bc.tx_ttl = 1

    # TTL-expired transaction from Alice
    priv_a, _ = the_block.generate_keypair()
    payload_a = the_block.RawTxPayload(
        from_="alice",
        to="bob",
        amount_consumer=1,
        amount_industrial=0,
        fee=1_000,
        pct_ct=100,
        nonce=1,
        memo=b"",
    )
    stx_a = the_block.sign_tx(list(priv_a), payload_a)
    bc.submit_transaction(stx_a)

    # Orphaned transaction from Carol (account removed)
    priv_c, _ = the_block.generate_keypair()
    payload_c = the_block.RawTxPayload(
        from_="carol",
        to="bob",
        amount_consumer=1,
        amount_industrial=0,
        fee=1_000,
        pct_ct=100,
        nonce=1,
        memo=b"",
    )
    stx_c = the_block.sign_tx(list(priv_c), payload_c)
    bc.submit_transaction(stx_c)
    bc.remove_account("carol")

    # Backdate Alice's entry so it expires on the next purge
    bc.backdate_mempool_entry("alice", 1, 0)

    # Snapshot metrics before starting the purge loop
    before = _parse_metrics(the_block.gather_metrics())

    flag = the_block.ShutdownFlag()
    handle = the_block.maybe_spawn_purge_loop(bc, flag)
    assert isinstance(handle, the_block.PurgeLoopHandle)
    time.sleep(2)
    flag.trigger()
    handle.join()
    del os.environ["TB_PURGE_LOOP_SECS"]

    # Verify metrics after the purge completes
    after = _parse_metrics(the_block.gather_metrics())

    assert after["ttl_drop_total"] == before["ttl_drop_total"] + 1
    assert after["orphan_sweep_total"] == before["orphan_sweep_total"] + 1
    assert after["mempool_size"] == 0
