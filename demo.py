"""Interactive demo showing basic blockchain operations."""

import os
import shutil
import the_block


def main():
    print("==> Initializing blockchain…")
    if os.path.exists("chain_db"):
        shutil.rmtree("chain_db")
    bc = the_block.Blockchain()
    # Lower difficulty for quick demo runs
    bc.difficulty = 8
    print(
        "A fresh chain database is created. Difficulty determines how many leading zero bits a block hash must have."
    )
    print(f"Difficulty set to {bc.difficulty}\n")

    print("==> Adding accounts: 'miner' and 'alice'…")
    # Each account maintains separate consumer and industrial balances
    bc.add_account("miner", 0, 0)
    bc.add_account("alice", 0, 0)
    print(
        "Accounts track two token balances: consumer and industrial. Both start at zero.\n"
    )

    print("==> Generating ed25519 keypair for miner…")
    # Keys authorize transactions and prove ownership of balances
    priv, pub = the_block.generate_keypair()
    print(f"Private key length: {len(priv)}")
    print(f"Public  key length: {len(pub)}\n")

    print("==> Signing & verifying a sample message…")
    msg = b"test transaction"
    # Sign bytes with the private key and immediately verify with the
    # corresponding public key to demonstrate the API
    sig = the_block.sign_message(priv, msg)
    assert the_block.verify_signature(pub, msg, sig), "Signature check failed"
    print("Signature valid. These keys will be used to authorize real transfers.\n")

    print("==> Mining genesis block for 'miner'…")
    # The first block initializes supply so later transfers have value
    block0 = bc.mine_block("miner")
    print(f"Block {block0.index} mined, hash = {block0.hash}")
    print(
        "The genesis block gives the miner an initial reward so there is currency in circulation."
    )
    m0 = bc.get_account_balance("miner")
    a0 = bc.get_account_balance("alice")
    print(f"miner balance:    consumer={m0.consumer}, industrial={m0.industrial}")
    print(f"alice balance:    consumer={a0.consumer}, industrial={a0.industrial}\n")

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
        fee_token=0,
        nonce=1,
        memo=b"",
    )
    # Sign the payload to produce a SignedTransaction struct
    stx = the_block.sign_tx(list(priv), payload)
    bc.submit_transaction(stx)
    print(f"Transaction queued (fee={fee}).\n")

    print(
        "==> Mining next block for 'miner' (collecting fee)… This requires solving a proof-of-work puzzle."
    )
    # Mining performs the hash puzzle and includes the queued transaction
    block1 = bc.mine_block("miner")
    print(f"Block {block1.index} mined, hash = {block1.hash}")
    m1 = bc.get_account_balance("miner")
    a1 = bc.get_account_balance("alice")
    print(f"miner balance:    consumer={m1.consumer}, industrial={m1.industrial}")
    print(f"alice balance:    consumer={a1.consumer}, industrial={a1.industrial}\n")

    print("==> Emission & reward state:")
    print(f" Block height:               {bc.block_height}")
    print(
        f" Current block reward:       {bc.block_reward_consumer} (consumer), {bc.block_reward_industrial} (industrial)"
    )
    em_c, em_i = bc.circulating_supply()
    print(f" Circulating supply:         {em_c} (consumer), {em_i} (industrial)\n")

    print("==> Mining 4 more blocks to demonstrate decay…")
    print("Each block's reward shrinks slightly as part of the monetary policy.")
    for _ in range(4):
        blk = bc.mine_block("miner")
        print(
            f" Block {blk.index}: next reward = {bc.block_reward_consumer} (consumer)"
        )

    # All done. The state on disk now contains 6 blocks.
    print("\n✅ All operations completed successfully.")


if __name__ == "__main__":
    main()
