"""Interactive walkthrough showcasing core features of The‑Block.

Run this script from a terminal to see how a fresh chain is created,
transactions are signed, and blocks are mined. Explanatory text is
printed at each step so no prior blockchain knowledge is required.

This script exercises the same production-grade APIs used by real
nodes, illustrating how wallet software can interact with the chain."""

import os
import shutil
import sys

import the_block


def explain(text: str) -> None:
    """Pretty printer used throughout the walkthrough."""
    print(text)


def main() -> None:
    # Start from a clean slate so results are reproducible.
    explain("==> Initializing blockchain…")
    if os.path.exists("chain_db"):
        shutil.rmtree("chain_db")

    explain(f"Network chain ID: {the_block.chain_id_py()}")
    bc = the_block.Blockchain.with_difficulty("chain_db", 8)
    explain(
        "A fresh database is ready. Difficulty controls how many leading zero bits a block hash must have."
    )
    explain(f"Difficulty set to {bc.difficulty}\n")

    print("==> Adding accounts: 'miner' and 'alice'…")
    # Each account maintains separate consumer and industrial balances

    bc.add_account("miner", 0, 0)
    bc.add_account("alice", 0, 0)
    explain("Accounts track two balances: consumer and industrial tokens.\n")

    print("==> Generating ed25519 keypair for miner…")

    priv: bytes
    pub: bytes
    priv, pub = the_block.generate_keypair()
    explain(f"Private key bytes: {len(priv)}, public key bytes: {len(pub)}\n")

    explain("==> Signing and verifying a sample message…")
    msg = b"test transaction"
    # Sign bytes with the private key and immediately verify with the
    # corresponding public key to demonstrate the API
    sig = the_block.sign_message(priv, msg)
    assert the_block.verify_signature(pub, msg, sig)
    explain("Signature valid. These keys will be used to authorize transfers.\n")

    print("==> Mining genesis block for 'miner'…")
    # The first block initializes supply so later transfers have value
    block0 = bc.mine_block("miner")
    print(f"Block {block0.index} mined, hash = {block0.hash}")
    print(
        "The genesis block gives the miner an initial reward so there is currency in circulation."
    )
    m0 = bc.get_account_balance("miner")
    a0 = bc.get_account_balance("alice")
    explain(f"miner balance: consumer={m0.consumer}, industrial={m0.industrial}")
    explain(f"alice balance: consumer={a0.consumer}, industrial={a0.industrial}\n")

    print(
        "==> Submitting a real transaction: miner → alice (1 consumer, 2 industrial, fee=3)"
    )
    amt_cons, amt_ind, fee = 1, 2, 3
    # Build the transaction payload using the Python bindings
    payload = the_block.RawTxPayload(
        from_="miner",
        to="alice",
        amount_consumer=amt_cons,
        amount_industrial=amt_ind,
        fee=fee,
        fee_selector=0,
        nonce=1,
        memo=b"",
    )
    stx = the_block.sign_tx(list(priv), payload)
    assert the_block.verify_signed_tx(stx)
    bc.submit_transaction(stx)
    explain("Transaction queued.\n")

    print(
        "==> Mining next block for 'miner' (collecting fee)… This requires solving a proof-of-work puzzle."
    )
    # Mining performs the hash puzzle and includes the queued transaction
    block1 = bc.mine_block("miner")
    assert bc.validate_block(block1)
    explain(f"Block {block1.index} mined with hash {block1.hash}")
    m1 = bc.get_account_balance("miner")
    a1 = bc.get_account_balance("alice")
    explain(f"miner balance: consumer={m1.consumer}, industrial={m1.industrial}")
    explain(f"alice balance: consumer={a1.consumer}, industrial={a1.industrial}\n")

    print("==> Emission & reward state:")
    print(f" Block height:               {bc.block_height}")
    print(
        f" Current block reward:       {bc.block_reward_consumer} (consumer), {bc.block_reward_industrial} (industrial)"
    )
    em_c, em_i = bc.circulating_supply()
    explain(f" Circulating supply:   {em_c} (consumer), {em_i} (industrial)\n")

    explain("==> Mining 4 more blocks to show reward decay…")
    for _ in range(4):
        blk = bc.mine_block("miner")
        explain(
            f" Block {blk.index} mined. Next reward will be {bc.block_reward_consumer} (consumer)"
        )

    # All done. The state on disk now contains 6 blocks.
    print("\n✅ All operations completed successfully.")


if __name__ == "__main__":
    try:
        main()
    except AssertionError as exc:
        print(f"Assertion failed: {exc}")
        sys.exit(1)
