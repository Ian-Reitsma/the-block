import the_block

print("==> Initializing blockchain...")
bc = the_block.Blockchain()
bc.difficulty = 8
print("Blockchain initialized. Difficulty set to 8.")

print("\n==> Adding account 'miner'...")
bc.add_account("miner", 0, 0)
balance = bc.get_account_balance("miner")
print(f"'miner' account balance: consumer={balance.consumer}, industrial={balance.industrial}")

print("\n==> Generating ed25519 keypair...")
priv, pub = the_block.generate_keypair()
print(f"Private key (len={len(priv)}): {priv}")
print(f"Public key (len={len(pub)}): {pub}")

print("\n==> Signing and verifying a sample message...")
msg = b"test transaction"
sig = the_block.sign_message(priv, msg)
verified = the_block.verify_signature(pub, msg, sig)
print(f"Signature: {sig}")
print(f"Signature valid? {'YES' if verified else 'NO'}")

print("\n==> Adding recipient 'alice' and sending a transaction...")
bc.add_account("alice", 0, 0)

# Build the *exact* message you pass to submit_transaction
msg_tx = (
    b"miner" +
    b"alice" +
    (0).to_bytes(8, "little") +
    (0).to_bytes(8, "little") +
    (0).to_bytes(8, "little")
)
sig_tx = the_block.sign_message(priv, msg_tx)
try:
    bc.submit_transaction(
        "miner", "alice", 0, 0, 0,
        list(pub),
        list(sig_tx)
    )
    print("Transaction submitted!")
except Exception as e:
    print("Transaction failed:", e)
sig_tx = the_block.sign_message(priv, msg_tx)
try:
    bc.submit_transaction(
        "miner", "alice", 0, 0, 0,
        list(pub),         # public_key as Vec<u8>
        list(sig_tx)       # signature as Vec<u8>
    )
    print("Transaction submitted!")
except Exception as e:
    print("Transaction failed:", e)

print("\n==> Mining a block...")
block = bc.mine_block()
print(f"Mined block index: {block.index}, hash: {block.hash}")

print("\n==> Updated balances after mining:")
print("miner:", bc.get_account_balance("miner").consumer, "(consumer)")
print("alice:", bc.get_account_balance("alice").consumer, "(consumer)")
print("\n==> Emission and reward state:")
print(f"Block height: {bc.block_height}")
print(f"Block reward (consumer): {bc.block_reward_consumer}")
print(f"Block reward (industrial): {bc.block_reward_industrial}")
em_cons, em_ind = bc.circulating_supply()
print(f"Circulating supply: consumer={em_cons}, industrial={em_ind}")

print("\n==> Mining several more blocks to see decay in action...")
for i in range(4):
    block = bc.mine_block()
    print(f"Block {block.index}: reward={bc.block_reward_consumer}, emission={bc.circulating_supply()[0]}")

