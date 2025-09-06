import time

import pytest
import the_block


def test_spawn_purge_loop_manual_interval(tmp_path):
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    handle = the_block.spawn_purge_loop(bc, 1, flag)
    assert isinstance(handle, the_block.PurgeLoopHandle)
    time.sleep(0.1)
    flag.trigger()
    handle.join()


def test_spawn_purge_loop_double_trigger_join(tmp_path):
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    handle = the_block.spawn_purge_loop(bc, 1, flag)
    time.sleep(0.1)
    flag.trigger()
    flag.trigger()
    handle.join()
    handle.join()


def test_spawn_purge_loop_panic_propagates(tmp_path):
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    bc.panic_next_purge()
    flag = the_block.ShutdownFlag()
    handle = the_block.spawn_purge_loop(bc, 1, flag)
    time.sleep(0.1)
    with pytest.raises(RuntimeError):
        handle.join()


def _parse_metrics(text: str) -> dict[str, int]:
    data: dict[str, int] = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) == 2 and parts[1].isdigit():
            data[parts[0]] = int(parts[1])
    return data


def test_spawn_purge_loop_concurrent(tmp_path):
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    bc.add_account("alice", 10_000, 0)
    bc.add_account("bob", 0, 0)
    bc.tx_ttl = 1

    priv, _ = the_block.generate_keypair()
    payload = the_block.RawTxPayload(
        from_="alice",
        to="bob",
        amount_consumer=1,
        amount_industrial=0,
        fee=1_000,
        pct_ct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)
    bc.backdate_mempool_entry("alice", 1, 0)

    metrics_before = None
    if hasattr(the_block, "gather_metrics"):
        metrics_before = _parse_metrics(the_block.gather_metrics())
        assert metrics_before["mempool_size"] == 1

    flag_a = the_block.ShutdownFlag()
    flag_b = the_block.ShutdownFlag()
    handle_a = the_block.spawn_purge_loop(bc, 1, flag_a)
    start_a = time.time()
    handle_b = the_block.spawn_purge_loop(bc, 2, flag_b)
    start_b = time.time()
    print(f"loop A start {start_a:.3f}; loop B start {start_b:.3f}")

    time.sleep(3)

    metrics_mid = None
    if metrics_before is not None:
        metrics_mid = _parse_metrics(the_block.gather_metrics())
        assert metrics_mid["mempool_size"] == 0

    flag_a.trigger()
    stop_a = time.time()
    print(f"loop A stop {stop_a:.3f}")
    time.sleep(0.05)
    flag_b.trigger()
    stop_b = time.time()
    print(f"loop B stop {stop_b:.3f}")

    # Join handles in opposite order and repeat join on A
    handle_b.join()
    if metrics_before is not None:
        after_b = _parse_metrics(the_block.gather_metrics())
        assert after_b["mempool_size"] == 0
    handle_a.join()
    handle_a.join()

    if metrics_before is not None:
        after = _parse_metrics(the_block.gather_metrics())
        assert after["mempool_size"] == 0
        assert after["ttl_drop_total"] == metrics_before.get("ttl_drop_total", 0) + 1


def test_spawn_purge_loop_overlap_reverse_trigger(tmp_path):
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    bc.add_account("alice", 10_000, 0)
    bc.add_account("bob", 0, 0)
    bc.tx_ttl = 1

    priv, _ = the_block.generate_keypair()
    payload = the_block.RawTxPayload(
        from_="alice",
        to="bob",
        amount_consumer=1,
        amount_industrial=0,
        fee=1_000,
        pct_ct=100,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)
    bc.backdate_mempool_entry("alice", 1, 0)

    metrics_before = None
    if hasattr(the_block, "gather_metrics"):
        metrics_before = _parse_metrics(the_block.gather_metrics())
        assert metrics_before["mempool_size"] == 1

    flag_a = the_block.ShutdownFlag()
    flag_b = the_block.ShutdownFlag()
    handle_a = the_block.spawn_purge_loop(bc, 1, flag_a)
    start_a = time.time()
    handle_b = the_block.spawn_purge_loop(bc, 1, flag_b)
    start_b = time.time()
    print(f"loop A start {start_a:.3f}; loop B start {start_b:.3f}")

    time.sleep(1.5)

    flag_b.trigger()
    stop_b = time.time()
    print(f"loop B stop {stop_b:.3f}")
    flag_a.trigger()
    stop_a = time.time()
    print(f"loop A stop {stop_a:.3f}")

    handle_a.join()
    if metrics_before is not None:
        after_a = _parse_metrics(the_block.gather_metrics())
        assert after_a["mempool_size"] == 0

    handle_b.join()
    if metrics_before is not None:
        after = _parse_metrics(the_block.gather_metrics())
        assert after["mempool_size"] == 0
        assert after["ttl_drop_total"] == metrics_before.get("ttl_drop_total", 0) + 1
