import os
import shutil
import the_block

def main():
    print("==> Initializing blockchain…")
    if os.path.exists("chain_db"):
        shutil.rmtree("chain_db")
    bc = the_block.Blockchain()
    bc.difficulty = 8
    print(f"Difficulty set to {bc.difficulty}\n")

    print("==> Adding accounts: 'miner' and 'alice'…")
    bc.add_account("miner", 0, 0)
    bc.add_account("alice", 0, 0)
    print("Accounts added.\n")

    print("==> Generating ed25519 keypair for miner…")
    priv, pub = the_block.generate_keypair()
    print(f"Private key length: {len(priv)}")
    print(f"Public  key length: {len(pub)}\n")

    print("==> Signing & verifying a sample message…")
    msg = b"test transaction"
    sig = the_block.sign_message(priv, msg)
    assert the_block.verify_signature(pub, msg, sig), "Signature check failed"
    print("Signature valid.\n")

    print("==> Mining genesis block for 'miner'…")
    block0 = bc.mine_block("miner")
    print(f"Block {block0.index} mined, hash = {block0.hash}")
    m0 = bc.get_account_balance("miner")
    a0 = bc.get_account_balance("alice")
    print(f"miner balance:    consumer={m0.consumer}, industrial={m0.industrial}")
    print(f"alice balance:    consumer={a0.consumer}, industrial={a0.industrial}\n")

    print("==> Submitting a real transaction: miner → alice (1 consumer, 2 industrial, fee=3)")
    amt_cons, amt_ind, fee = 1, 2, 3
    tx_msg = (
        b"miner" +
        b"alice" +
        amt_cons.to_bytes(8, "little") +
        amt_ind.to_bytes(8, "little") +
        fee.to_bytes(8, "little")
    )
    sig_tx = the_block.sign_message(priv, tx_msg)
    bc.submit_transaction(
        "miner", "alice",
        amt_cons, amt_ind, fee,
        list(pub), list(sig_tx)
    )
    print(f"Transaction queued (fee={fee}).\n")

    print("==> Mining next block for 'miner' (collecting fee)…")
    block1 = bc.mine_block("miner")
    print(f"Block {block1.index} mined, hash = {block1.hash}")
    m1 = bc.get_account_balance("miner")
    a1 = bc.get_account_balance("alice")
    print(f"miner balance:    consumer={m1.consumer}, industrial={m1.industrial}")
    print(f"alice balance:    consumer={a1.consumer}, industrial={a1.industrial}\n")

    print("==> Emission & reward state:")
    print(f" Block height:               {bc.block_height}")
    print(f" Current block reward:       {bc.block_reward_consumer} (consumer), {bc.block_reward_industrial} (industrial)")
    em_c, em_i = bc.circulating_supply()
    print(f" Circulating supply:         {em_c} (consumer), {em_i} (industrial)\n")

    print("==> Mining 4 more blocks to demonstrate decay…")
    for _ in range(4):
        blk = bc.mine_block("miner")
        print(f" Block {blk.index}: next reward = {bc.block_reward_consumer} (consumer)")

    print("\n✅ All operations completed successfully.")

if __name__ == "__main__":
    main()
