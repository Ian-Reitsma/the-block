import os
import pytest
import the_block


def test_invalid_env_raises(tmp_path):
    os.environ['TB_PURGE_LOOP_SECS'] = 'abc'
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    with pytest.raises(ValueError) as exc:
        the_block.maybe_spawn_purge_loop(bc, flag)
    assert 'TB_PURGE_LOOP_SECS' in str(exc.value)
    del os.environ['TB_PURGE_LOOP_SECS']


def test_zero_env_raises(tmp_path):
    os.environ['TB_PURGE_LOOP_SECS'] = '0'
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    with pytest.raises(ValueError):
        the_block.maybe_spawn_purge_loop(bc, flag)
    del os.environ['TB_PURGE_LOOP_SECS']


def test_missing_env_raises(tmp_path):
    if 'TB_PURGE_LOOP_SECS' in os.environ:
        del os.environ['TB_PURGE_LOOP_SECS']
    bc = the_block.Blockchain.with_difficulty(str(tmp_path), 1)
    flag = the_block.ShutdownFlag()
    with pytest.raises(ValueError):
        the_block.maybe_spawn_purge_loop(bc, flag)
