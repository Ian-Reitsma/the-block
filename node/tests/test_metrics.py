import os
import time

import the_block
import pytest


def _parse_metrics(text: str) -> dict[str, int]:
    data: dict[str, int] = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) == 2 and parts[1].isdigit():
            data[parts[0]] = int(parts[1])
    return data


def test_purge_loop_metrics(tmp_path):
    if not hasattr(the_block, "gather_metrics"):
        pytest.skip("telemetry not enabled")
    os.environ["TB_PURGE_LOOP_SECS"] = "1"
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    bc.add_account("alice", 10_000, 0)
    bc.add_account("bob", 0, 0)
    bc.add_account("carol", 10_000, 0)
    bc.tx_ttl = 1

    priv_a, _ = the_block.generate_keypair()
    payload_a = the_block.RawTxPayload(
        from_="alice",
        to="bob",
        amount_consumer=1,
        amount_industrial=0,
        fee=1000,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx_a = the_block.sign_tx(list(priv_a), payload_a)
    bc.submit_transaction(stx_a)

    priv_c, _ = the_block.generate_keypair()
    payload_c = the_block.RawTxPayload(
        from_="carol",
        to="bob",
        amount_consumer=1,
        amount_industrial=0,
        fee=1000,
        pct=100,
        nonce=1,
        memo=b"",
    )
    stx_c = the_block.sign_tx(list(priv_c), payload_c)
    bc.submit_transaction(stx_c)
    del bc.accounts["carol"]

    before = _parse_metrics(the_block.gather_metrics())
    flag = the_block.ShutdownFlag()
    handle = the_block.maybe_spawn_purge_loop(bc, flag)
    assert handle is not None
    time.sleep(2)
    flag.trigger()
    handle.join()
    del os.environ["TB_PURGE_LOOP_SECS"]
    after = _parse_metrics(the_block.gather_metrics())

    # Purging should drop one or two expired transactions depending on internal ordering.
    delta = after["ttl_drop_total"] - before.get("ttl_drop_total", 0)
    assert 1 <= delta <= 2
