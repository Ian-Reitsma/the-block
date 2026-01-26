# Consensus Safety & Liveness Contract

This document is the executable specification for the current federated consensus path. The code must satisfy these properties; if it does not, the code is wrong.

## Model
- Validators with stake weights (UNL) vote on block hashes.
- Finality threshold: `stake_for(hash) >= 2/3 * total_stake`.
- Conflicting votes from the same validator are treated as **equivocation** and their stake is removed from consideration until governance explicitly refreshes the UNL.
- Rollback is operator-invoked only; it clears pending votes and equivocation flags.

## Promised Properties
1) **Safety (conflict-free finality):** No two distinct block hashes can both finalize unless >1/3 of total stake equivocate.
2) **Liveness under partial partitions:** If at least 2/3 stake can eventually communicate, some block will finalize once their votes are delivered (delivery may be delayed/jittered).
3) **Equivocation accountability:** Any validator that emits conflicting votes is marked faulty and its stake is excluded from future tallies until the UNL is refreshed.
4) **Auditable state transitions:** The voting state (votes, equivocators, finalized hash, total stake) is snapshot-able for tests, telemetry, and incident review.

## Executable Tests & Simulations
- `node/tests/consensus_finality.rs` validates equivocation handling and rollback semantics.
- `node/tests/pos_finality.rs` validates stake-weighted finality and partition healing (50/50 split cannot finalize; converging votes do).
- `node/tests/consensus_wan.rs` simulates WAN jitter/partial delivery to ensure conflicting forks do not finalize before a 2/3 supermajority converges.

## WAN/Partition Chaos Expectations
- Message delay/jitter alone must not permit conflicting finality.
- Partitions with <2/3 stake on any side must stall finality until connectivity heals.
- Once a supermajority is reachable, finality must occur without requiring perfect ordering of messages.

## Reorg & Rollback Story
- Finality is monotonic: once a hash is finalized, later votes can only reaffirm it.
- Operator-triggered rollback clears votes/equivocations, after which a new finalized hash can be established (e.g., post-incident).

## Implementation Pointers
- Finality gadget: `node/src/consensus/finality.rs`
- UNL/stake bookkeeping: `node/src/consensus/unl.rs`
- Tests: `node/tests/consensus_finality.rs`, `node/tests/pos_finality.rs`, `node/tests/consensus_wan.rs`
