# Bridge Primitives and Relayer Workflow

The bridge subsystem moves value between The‑Block and external chains without introducing custodial risk. This document describes the current lock/unlock implementation, relayer proof format, CLI flows, and outstanding work.

## Architecture Overview

1. **Lock Phase**
   - Users invoke `blockctl bridge deposit --amount <amt> --dest <chain>`.
   - The transaction locks funds in the on-chain `Bridge` contract and emits an event containing the deposit ID and destination chain.
2. **Relayer Proof**
   - Off-chain relayers watch the event stream and submit a Merkle proof to the destination chain.
   - Proofs include the deposit ID, amount, source account, and the BLAKE3 commitment of the lock event.
3. **Unlock Phase**
   - Once the destination chain verifies the proof, relayers call `blockctl bridge withdraw --id <deposit-id>` on The‑Block.
   - The contract validates that the deposit is unspent and releases the locked balance to the caller.

All bridge state lives under `state/bridges/` and survives restarts via bincode snapshots.

## Relayer Proof Format

```text
struct LockProof {
    deposit_id: u64,
    amount: u64,
    source: [u8; 32],
    dest_chain: u16,
    merkle_path: Vec<[u8;32]>,
}
```

Relayers must sign the serialized `LockProof` with their Ed25519 key. The contract verifies:

- signature matches a whitelisted relayer,
- `deposit_id` exists and is still locked,
- Merkle path recomputes the event root.

## CLI Examples

Lock funds on The‑Block destined for Ethereum:

```bash
blockctl bridge deposit \
  --from alice \
  --amount 50 \
  --dest-chain 1            # Ethereum chain ID
```

After the lock is observed and proven on Ethereum, unlock back on The‑Block:

```bash
blockctl bridge withdraw \
  --id 42 \
  --proof proof.json \
  --relayer-signature <hex>
```

`proof.json` is the canonical JSON form of `LockProof` above. The CLI converts the JSON into bincode before submitting the transaction.

## Outstanding Work

- **Light-Client Verification** – embed an Ethereum light client so The‑Block verifies destination headers directly.
- **Relayer Incentives** – fee market and slashing for misbehavior.
- **Multi-Asset Support** – extend the lock contract to wrap arbitrary tokens with minted representations on the destination chain.

Progress: 20%
