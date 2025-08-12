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
    if not hasattr(the_block, "gather_metrics"):
        pytest.skip("telemetry not enabled")

    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag1 = the_block.ShutdownFlag()
    flag2 = the_block.ShutdownFlag()
    handle1 = the_block.spawn_purge_loop(bc, 1, flag1)
    handle2 = the_block.spawn_purge_loop(bc, 1, flag2)
    time.sleep(0.1)
    before = _parse_metrics(the_block.gather_metrics())

    flag1.trigger()
    flag2.trigger()
    handle1.join()
    handle2.join()

    after = _parse_metrics(the_block.gather_metrics())
    assert before["mempool_size"] == after["mempool_size"] == 0
