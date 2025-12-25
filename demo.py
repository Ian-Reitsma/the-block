"""Interactive walkthrough for The‑Block's vision: a one‑second Layer 1 that
anchors micro‑shards and rewards honest service with dual Consumer/Industrial
tokens."""

from __future__ import annotations

import importlib
import os
import pathlib
import atexit
import random
import shutil
import subprocess
import sys
import time
import argparse
import json
import urllib.request


if os.getenv("TB_SAVE_LOGS") == "1":
    log_dir = pathlib.Path("demo_logs")
    log_dir.mkdir(exist_ok=True)
    _out = open(log_dir / "stdout.log", "w")
    _err = open(log_dir / "stderr.log", "w")
    atexit.register(_out.close)
    atexit.register(_err.close)
    sys.stdout = _out
    sys.stderr = _err


def _load_the_block():
    """Import the_block, building it on demand if needed."""
    try:
        return importlib.import_module("the_block")
    except ModuleNotFoundError as exc:
        # Print to stdout so the user and tests can see the message
        print("the first-party python bridge is not yet available; rerun the demo once the `python-bindings` feature ships")
        raise SystemExit(1) from exc


the_block = _load_the_block()

MAX_FEE = (1 << 63) - 1
BASE_FEE = 1_000
MAX_SUPPLY_CONSUMER = 20_000_000_000_000
MAX_SUPPLY_INDUSTRIAL = 20_000_000_000_000
DECAY_NUMERATOR = 99_995
DECAY_DENOMINATOR = 100_000

ENV_PREPARED = False


def explain(msg: str) -> None:
    """Print a human-friendly line."""
    print(msg)


def require(cond: bool, *, msg: str, context: dict | None = None) -> None:
    """Log failure context and exit with non-zero code."""
    if cond:
        return
    ctx = " ".join(f"{k}={v}" for k, v in (context or {}).items())
    if ctx:
        explain(f"Assertion failed: {msg} ({ctx})")
    else:
        explain(f"Assertion failed: {msg}")
    raise SystemExit(1)


def metric_val(metrics: str, name: str) -> int:
    """Extract integer value for a metric name from a metrics string."""
    for line in metrics.splitlines():
        if line.startswith(name):
            try:
                return int(line.rsplit(" ", 1)[-1])
            except ValueError:
                return 0
    return 0


def _rpc_call(url: str, payload: dict) -> dict:
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode())


def difficulty_quic_demo(enable_quic: bool) -> None:
    """Spawn a node, mine a few blocks, and report difficulty via RPC."""
    explain(
        "Starting node subprocess" + (" with QUIC enabled" if enable_quic else "")
    )
    cmd = [
        "cargo",
        "run",
        "-p",
        "the_block",
        "--bin",
        "node",
        "--",
        "run",
        "--rpc-addr",
        "127.0.0.1:3030",
        "--mempool-purge-interval",
        "0",
    ]
    if enable_quic:
        cmd.append("--quic")
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        time.sleep(2)
        _rpc_call(
            "http://127.0.0.1:3030",
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "start_mining",
                "params": {"miner": "demo", "nonce": 1},
            },
        )
        last = None
        for i in range(5):
            res = _rpc_call(
                "http://127.0.0.1:3030",
                {
                    "jsonrpc": "2.0",
                    "id": i + 2,
                    "method": "consensus.difficulty",
                    "params": {},
                },
            )
            diff = res.get("result", {}).get("difficulty")
            if diff is not None and diff != last:
                explain(f"current difficulty: {diff}")
                last = diff
            time.sleep(1)
        _rpc_call(
            "http://127.0.0.1:3030",
            {
                "jsonrpc": "2.0",
                "id": 99,
                "method": "stop_mining",
                "params": {"miner": "demo", "nonce": 2},
            },
        )
    finally:
        proc.terminate()
        proc.wait()


def show_pending(bc: the_block.Blockchain, sender: str, recipient: str) -> None:
    """Display pending reservations for two accounts."""
    s = bc.accounts[sender]
    r = bc.accounts[recipient]
    explain(
        f"{sender} pending -> c={s.pending_consumer} i={s.pending_industrial} n={s.pending_nonce}"
    )
    explain(
        f"{recipient} pending -> c={r.pending_consumer} i={r.pending_industrial} n={r.pending_nonce}"
    )


def init_environment() -> None:
    """Ensure deterministic behaviour and a clean database."""
    global ENV_PREPARED
    explain("Preparing deterministic environment")
    os.environ["PYTHONHASHSEED"] = "0"
    random.seed(0)
    explain("Python random seeded with 0")
    if "TB_PURGE_LOOP_SECS" not in os.environ:
        os.environ["TB_PURGE_LOOP_SECS"] = "1"
        explain("TB_PURGE_LOOP_SECS unset; defaulting to 1 second purge interval")
    if os.path.exists("chain_db"):
        shutil.rmtree("chain_db")
        explain("Removed previous chain_db directory")
    ENV_PREPARED = True


def init_chain() -> the_block.Blockchain:
    """Create a blockchain with trivial proof of work."""
    require(ENV_PREPARED, msg="environment not initialised")
    require(not os.path.exists("chain_db"), msg="stale chain_db present")
    explain("Creating new blockchain with difficulty 1")
    bc = the_block.Blockchain.with_difficulty("chain_db", 1)
    bc.genesis_block()
    explain("Genesis block created; chain starts at height 0")
    explain(f"Chain length now {bc.current_chain_length()}")
    return bc


def create_accounts(bc: the_block.Blockchain) -> list[str]:
    """Prepare user accounts used throughout the demo."""
    explain("Creating four demo accounts: miner, alice, bob, faucet")
    accounts = ["miner", "alice", "bob", "faucet"]
    for name in accounts:
        bc.add_account(name, 0, 0)
        bal = bc.get_account_balance(name)
        explain(
            f"Account {name} starts with consumer={bal.consumer} and "
            f"industrial={bal.industrial}"
        )
    return accounts


def keypair_demo() -> bytes:
    """Generate a keypair and prove signing works."""
    explain("Generating fresh Ed25519 keypair; keys differ each run")
    priv, pub = the_block.generate_keypair()
    explain(f"Public key: {pub.hex()}")
    message = b"hello"
    sig = the_block.sign_message(priv, message)
    if the_block.verify_signature(pub, message, sig):
        explain("Signature verified; cryptography working")
    return priv


def fee_demo() -> None:
    """Show fee split and error handling."""
    explain("Exploring fee selectors; The-Block splits fees across two token pools")
    for sel in (0, 1, 2):
        for fee in (0, 1, 9, MAX_FEE):
            fc, fi = the_block.fee_decompose(sel, fee)
            explain(
                f"Selector {sel} with fee {fee} -> " f"consumer {fc}, industrial {fi}"
            )
    try:
        the_block.fee_decompose(3, 1)
    except the_block.ErrInvalidSelector:
        explain("Selector 3 rejected: invalid selector")
    try:
        the_block.fee_decompose(0, MAX_FEE + 1)
    except the_block.ErrFeeOverflow:
        explain("Fee overflow rejected: value exceeds allowed range")


def decode_payload_demo() -> None:
    """Round-trip canonical payload bytes through decode_payload."""
    explain("Decoding canonical payload bytes")
    payload = the_block.RawTxPayload(
        from_="alice",
        to="bob",
        amount_consumer=1,
        amount_industrial=0,
        fee=BASE_FEE,
        pct_ct=50,
        nonce=0,
        memo=b"demo",
    )
    raw = the_block.canonical_payload(payload)
    decoded = the_block.decode_payload(raw)
    explain(
        f"Decoded payload -> from={getattr(decoded, 'from')} to={decoded.to} "
        f"consumer={decoded.amount_consumer} fee={decoded.fee}"
    )


def mine_initial_block(bc: the_block.Blockchain, accounts: list[str]) -> None:
    """Mine one block so the miner earns starting funds."""
    explain("Mining first block so miner receives starting tokens")
    blk = bc.mine_block("miner")
    explain(f"Mined block #{blk.index} with hash {blk.hash}")
    check_supply(bc, accounts)


def transaction_errors(bc: the_block.Blockchain, priv: bytes) -> None:
    """Demonstrate fee selectors and nonce failure paths."""
    explain("Nonce is like a check number: use each once and in order")
    next_nonce = 1
    routes = {
        100: "all fee to consumer token",
        0: "all fee to industrial token",
        50: "fee split between tokens",
    }
    for sel, note in routes.items():
        payload = the_block.RawTxPayload(
            from_="miner",
            to="alice",
            amount_consumer=1,
            amount_industrial=0,
            fee=BASE_FEE,
            pct_ct=sel,
            nonce=next_nonce,
            memo=b"selector demo",
        )
        stx = the_block.sign_tx(list(priv), payload)
        bc.submit_transaction(stx)
        explain(f"Selector {sel}: {note}")
        next_nonce += 1
    reuse_payload = the_block.RawTxPayload(
        from_="miner",
        to="alice",
        amount_consumer=1,
        amount_industrial=0,
        fee=BASE_FEE,
        pct_ct=100,
        nonce=2,
        memo=b"reused nonce",
    )
    try:
        stx_dup = the_block.sign_tx(list(priv), reuse_payload)
        bc.submit_transaction(stx_dup)
    except the_block.ErrDuplicateTx:
        explain(
            "Reusing nonce 2 is like writing two checks with the same number; the bank rejects it"
        )
    show_pending(bc, "miner", "alice")


def mine_blocks(bc: the_block.Blockchain, accounts: list[str], priv: bytes) -> None:
    """Mine three blocks and show state after each one."""
    explain("Mining three blocks to show transaction inclusion and rewards")
    for i in range(3):
        payload = the_block.RawTxPayload(
            from_="miner",
            to="bob",
            amount_consumer=1,
            amount_industrial=0,
            fee=BASE_FEE,
            pct_ct=50,
            nonce=i + 4,
            memo=b"block transfer",
        )
        stx = the_block.sign_tx(list(priv), payload)
        bc.submit_transaction(stx)
        explain("Transaction queued for inclusion")
        show_pending(bc, "miner", "bob")
        blk = bc.mine_block("miner")
        explain(f"Mined block #{blk.index} with hash {blk.hash}")
        check_supply(bc, accounts)
        tot_c, tot_i = bc.circulating_supply()
        explain(f"Circulating totals -> consumer {tot_c}, industrial {tot_i}")


def emission_cap_demo(bc: the_block.Blockchain, accounts: list[str]) -> None:
    """Reach the emission cap on the final mined block."""
    explain("Demonstrating emission cap enforcement")
    next_c = (bc.block_reward_consumer.value * DECAY_NUMERATOR) // DECAY_DENOMINATOR
    next_i = (bc.block_reward_industrial.value * DECAY_NUMERATOR) // DECAY_DENOMINATOR
    sum_c = sum(bc.get_account_balance(a).consumer for a in accounts)
    sum_i = sum(bc.get_account_balance(a).industrial for a in accounts)
    bc.emission_consumer = MAX_SUPPLY_CONSUMER - next_c
    bc.emission_industrial = MAX_SUPPLY_INDUSTRIAL - next_i
    filler_c = bc.emission_consumer - sum_c
    filler_i = bc.emission_industrial - sum_i
    bc.add_account("cap_filler", filler_c, filler_i)
    accounts.append("cap_filler")
    supply_before = bc.circulating_supply()
    blk = bc.mine_block("miner")
    explain(f"Mined block #{blk.index} reaching cap")
    supply_after = bc.circulating_supply()
    require(
        supply_after
        == (
            MAX_SUPPLY_CONSUMER,
            MAX_SUPPLY_INDUSTRIAL,
        ),
        msg="emission cap mismatch",
        context={"block_index": blk.index, "nonce": blk.nonce},
    )
    explain(
        f"Supply before {supply_before}, after {supply_after}; remaining emission consumed, cap reached"
    )
    check_supply(bc, accounts)


def fee_split_demo() -> None:
    """Show fee splitting between CT and IT."""
    explain("Demonstrating fee splitting via pct_ct")
    payload = the_block.RawTxPayload(
        from_="alice",
        to="bob",
        amount_consumer=0,
        amount_industrial=0,
        nonce=0,
        pct_ct=50,
        fee=1,
    )
    raw = the_block.canonical_payload(payload)
    decoded = the_block.decode_payload(raw)
    explain(f"encoded pct_ct=50, decoded pct_ct={decoded.pct_ct}")


def escrow_demo() -> None:
    """Show basic escrow flow on the DEX."""
    explain("Escrow example: lock funds and release a payment")
    escrow = {"locked": 100, "released": 0}
    escrow["released"] += 40
    explain(f"released 40, remaining {escrow['locked'] - escrow['released']}")


def compute_job_demo() -> None:
    """Submit a sample compute job."""
    explain("Compute job submission: estimate workload units")
    data = b"demo workload"
    units = (len(data) + 1_048_575) // 1_048_576
    explain(f"{len(data)} bytes -> {units} compute unit")


def restart_purge_demo(priv: bytes) -> None:
    """Submit expiring tx, restart, and verify purge & metrics."""
    if not hasattr(the_block, "gather_metrics"):
        explain("Build with `--features telemetry` to run metric assertions")
        return
    explain("Demonstrating TTL purge on restart")
    path = "ttl_chain"
    if os.path.exists(path):
        shutil.rmtree(path)
        explain("Removed previous ttl_chain directory")
    bc = the_block.Blockchain.with_difficulty(path, 1)
    bc.genesis_block()
    bc.add_account("miner", 1000, 1000)
    bc.add_account("alice", 0, 0)
    bc.tx_ttl = 1
    payload = the_block.RawTxPayload(
        from_="miner",
        to="alice",
        amount_consumer=1,
        amount_industrial=0,
        fee=BASE_FEE,
        pct_ct=50,
        nonce=1,
        memo=b"expire",
    )
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)
    explain("Submitted expiring transaction; persisting and waiting")
    metrics_before = the_block.gather_metrics()
    ttl_before = metric_val(metrics_before, "ttl_drop_total")
    startup_before = metric_val(metrics_before, "startup_ttl_drop_total")
    bc.persist_chain()
    time.sleep(1.1)  # wait just over 1s for TTL expiry
    old_ttl = os.environ.get("TB_MEMPOOL_TTL_SECS")
    os.environ["TB_MEMPOOL_TTL_SECS"] = "1"
    try:
        bc = the_block.Blockchain.open(path)
        after = the_block.gather_metrics()
        ttl_after = metric_val(after, "ttl_drop_total")
        startup_after = metric_val(after, "startup_ttl_drop_total")
        mempool_size = metric_val(after, "mempool_size")
        explain(
            f"TTL_DROP_TOTAL before {ttl_before}, after {ttl_after}; "
            f"STARTUP_TTL_DROP_TOTAL before {startup_before}, after {startup_after}; "
            f"mempool_size={mempool_size}"
        )
        require(
            ttl_after == ttl_before + 1,
            msg="TTL_DROP_TOTAL did not increment",
            context={"before": ttl_before, "after": ttl_after},
        )
        require(
            startup_after == startup_before + 1,
            msg="STARTUP_TTL_DROP_TOTAL did not increment",
            context={"before": startup_before, "after": startup_after},
        )
        require(
            mempool_size == 0,
            msg="mempool not empty",
            context={"mempool_size": mempool_size},
        )
    finally:
        if old_ttl is None:
            os.environ.pop("TB_MEMPOOL_TTL_SECS", None)
        else:
            os.environ["TB_MEMPOOL_TTL_SECS"] = old_ttl
        shutil.rmtree(path, ignore_errors=True)


def persistence_demo(bc: the_block.Blockchain) -> None:
    """Illustrate persistence call and re-open chain."""
    explain("Persisting chain state to disk")
    bc.persist_chain()
    bc2 = the_block.Blockchain.with_difficulty("chain_db", 1)
    bc2.genesis_block()
    explain(
        "Reopened chain for persistence demo; in this prototype the "
        "state resets to genesis"
    )


def cleanup() -> None:
    """Remove database so repeated runs start fresh."""
    explain("Cleaning up chain_db directory")
    shutil.rmtree("chain_db", ignore_errors=True)


def check_supply(bc: the_block.Blockchain, accounts: list[str]) -> None:
    """Check supply caps and balance sums."""
    tot_c, tot_i = bc.circulating_supply()
    sum_c = sum(bc.get_account_balance(a).consumer for a in accounts)
    sum_i = sum(bc.get_account_balance(a).industrial for a in accounts)
    assert tot_c <= MAX_SUPPLY_CONSUMER, "consumer supply exceeds cap"
    assert tot_i <= MAX_SUPPLY_INDUSTRIAL, "industrial supply exceeds cap"
    assert (sum_c, sum_i) == (tot_c, tot_i), "balance mismatch"


def demo_steps(bc: the_block.Blockchain) -> None:
    """Run the core demo steps once a purge loop is active."""
    accounts = create_accounts(bc)
    priv = keypair_demo()
    fee_demo()
    decode_payload_demo()
    mine_initial_block(bc, accounts)
    transaction_errors(bc, priv)
    mine_blocks(bc, accounts, priv)
    emission_cap_demo(bc, accounts)
    fee_split_demo()
    escrow_demo()
    compute_job_demo()
    restart_purge_demo(priv)
    persistence_demo(bc)


def main() -> None:
    """Run the full demo sequentially."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--max-runtime", type=int, default=20)
    parser.add_argument(
        "--quic",
        action="store_true",
        help="start the node with QUIC enabled for the RPC demo",
    )
    args = parser.parse_args()
    start = time.time()
    init_environment()
    bc = init_chain()
    if os.getenv("TB_DEMO_MANUAL_PURGE"):
        explain("TB_DEMO_MANUAL_PURGE set: demonstrating manual purge-loop control")
        flag = the_block.ShutdownFlag()
        explain("ShutdownFlag created; trigger it like a fuse to stop the loop")
        handle = the_block.spawn_purge_loop(bc, 1, flag)
        explain("Handle returned; join waits for the loop to finish")
        try:
            demo_steps(bc)
            difficulty_quic_demo(args.quic)
        finally:
            explain("Triggering shutdown flag and joining purge loop")
            flag.trigger()
            handle.join()
    else:
        # TB_PURGE_LOOP_SECS controls purge interval for the context manager.
        with the_block.PurgeLoop(bc):
            demo_steps(bc)
            difficulty_quic_demo(args.quic)
    cleanup()
    explain("Demo complete")
    if time.time() - start > args.max_runtime:
        explain("Max runtime exceeded")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
