# Bridge Primitives and Light-Client Workflow

The bridge subsystem moves value between The‑Block and external chains without introducing custodial risk. This document describes the lock/unlock implementation, light‑client header verification, relayer proof format, CLI flows, and outstanding work.

## Architecture Overview

1. **Lock Phase**
 - Users invoke `blockctl bridge deposit --amount <amt> --dest <chain>`.
  - The transaction locks funds in the on-chain `Bridge` contract and emits an event containing the deposit ID, destination chain, and current partition marker so downstream relayers can avoid isolated shards.
2. **Relayer Proof**
   - Off-chain relayers watch the event stream and submit a Merkle proof to the destination chain.
   - Proofs include the deposit ID, amount, source account, and the BLAKE3 commitment of the lock event.
3. **Unlock Phase**
   - Once the destination chain verifies the proof, relayers call `blockctl bridge withdraw --id <deposit-id>` on The‑Block.
   - The contract validates that the deposit is unspent and releases the locked balance to the caller.

All bridge state lives under the `SimpleDb` tree (`state/bridges/`) so channel balances, relayer sets, and pending withdrawals survive restarts and reload automatically.

## Light-Client Header Verification

`verify_header` validates external chain headers and Merkle proofs before minting mirrored tokens.

```rust
struct Header {
    chain_id: String,
    height: u64,
    merkle_root: [u8;32],
    signature: [u8;32], // blake3(chain_id || height || merkle_root)
}

struct Proof {
    leaf: [u8;32],
    path: Vec<[u8;32]>,
}
```

Sequence:

1. Relayers fetch an external `Header` and Merkle `Proof` for the deposit event.
2. `blockctl bridge deposit --header header.json --proof proof.json` calls the `bridge.verify_deposit` RPC which forwards to `Bridge::deposit_with_relayer`.
3. `deposit_with_relayer` invokes `verify_pow` and `light_client::verify`, credits the user on success, and persists the full header JSON under `state/bridge_headers/<hash>.json` to prevent replay and allow audit.
4. Telemetry counters `bridge_proof_verify_success_total` and `bridge_proof_verify_failure_total` track verification results.

Sample `header.json` and `proof.json` files reside in `examples/bridges/` for development testing.

The `state/bridge_headers/` directory stores one file per verified header. Each
entry contains the serialised `Header` plus the block height that introduced it.
Schema migration details live in
[`docs/schema_migrations/v8_bridge_headers.md`](schema_migrations/v8_bridge_headers.md).

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

| Field       | Type        | Example file |
|-------------|-------------|--------------|
| `deposit_id`| `u64`       | `examples/bridges/proof.json` |
| `amount`    | `u64`       | `examples/bridges/proof.json` |
| `source`    | `[u8;32]`   | `examples/bridges/header.json` |
| `dest_chain`| `u16`       | `examples/bridges/proof.json` |
| `merkle_path`| `Vec<[u8;32]>` | `examples/bridges/proof.json` |

## Relayer Workflow & Incentives

Relayers stake native tokens to participate in bridge operations. Each `Relayer` maintains a bonded `stake` and a `slashes` counter. Deposits now require a quorum of approvals: the `bridge.verify_deposit` RPC accepts a `RelayerBundle` containing multiple proofs and validates that at least `BridgeConfig::relayer_quorum` entries check out while matching persisted shard affinity.

1. Each proof in the bundle is recomputed; invalid signers are slashed immediately and surfaced in `bridge_slashes_total`.
2. `PowHeader` encapsulates an external header and lightweight PoW target; `verify_deposit` rejects headers that fail the `verify_pow` check.
3. The Merkle path is validated and the header JSON recorded to prevent replays.

Invalid submissions increment `bridge_invalid_proof_total`, slash one unit of stake, and bump both the `relayer_slash_total` and `bridge_slashes_total` counters. Operators can query current collateral via `bridge.relayer_status` and inspect pending withdrawals through the explorer's `bridge_challenges` view.

`BridgeConfig` exposes per-chain settings such as `confirm_depth` and `fee_per_byte`, allowing runtime tuning without recompilation.

## CLI Examples

Lock funds on The‑Block using a light-client proof:

```bash
blockctl bridge deposit \
  --user alice \
  --amount 50 \
  --header header.json \
  --proof proof.json
```

After the lock is observed and proven on Ethereum, unlock back on The‑Block using a multi-relayer proof bundle. Withdrawals enter a challenge window; provide the relayer list up front and monitor the returned commitment:

```bash
blockctl bridge withdraw \
  --user alice \
  --amount 50 \
  --relayers r1,r2
```

If a challenge is required, submit it with the commitment hash returned by the CLI:

```bash
blockctl bridge challenge --commitment <hex>
```

Operators can also monitor the live bridge ledger via:

```bash
blockctl bridge pending --asset native
blockctl bridge challenges
blockctl bridge relayers --asset native
blockctl bridge history --asset native --limit 20
blockctl bridge slash-log
```

Relayer bonds can be provisioned off-chain and topped up through the RPC by calling
`blockctl bridge bond --relayer <id> --amount <tokens>`.

`header.json` and `proof.json` follow the formats above and are consumed directly by the CLI.

## Outstanding Work

- **Multi-Asset Support** – extend the lock contract to wrap arbitrary tokens with minted representations on the destination chain.

Progress: 74%

## Dispute Resolution & Threat Model

The node now persists per-asset bridge channels in a sled-backed database so that lock
balances, pending withdrawals, and relayer collateral all survive process restarts and
chain rollbacks. Deposits record their originating proof metadata with monotonically
increasing nonces to prevent replay; receipts can be paged via `bridge.deposit_history`
and exported for audit.

Withdrawals enter a challenge window (default 30 seconds) where any operator can invoke
`bridge.challenge_withdrawal`. Challenged releases immediately re-credit the user’s
locked balance, mark the receipt for review, and slash every relayer that signed the
bundle. Collateral is debited from the bond ledger and the slashing event appears under
`bridge.slash_log`. Successful releases require a governance attestation: the node calls
`governance::ensure_release_authorized("bridge:<asset>:<commitment>")` before honouring a
withdrawal, guaranteeing that signer thresholds are enforced alongside the relayer quorum.

Telemetry counters `bridge_challenges_total` and `bridge_slashes_total` expose these
events for dashboards, while CLI helpers allow operators to enumerate active challenges
and relayer quorum composition. The threat model assumes at least one honest challenger
per channel during the dispute window; even if a malicious quorum attempts to withdraw
forged funds, bonded relayers are penalised and the audited receipts provide clear
evidence for governance intervention.
