# DEX AMM Pools
> **Review (2025-09-23):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

This module implements constant-product automated market maker (AMM) pools for CT pairs (CT against a paired asset such as USDC or a synthetic test reserve). Liquidity providers deposit both sides of the pair and receive pool shares representing their claim on the reserves. Swaps maintain the invariant `x * y = k`.

## Liquidity

- **Add liquidity**: deposit CT and the paired asset in proportion to current reserves to mint shares.
- **Remove liquidity**: burn shares to withdraw proportional reserves from both sides.
Wallet CLI exposes `liquidity add` and `liquidity remove` commands for these flows.

## Swaps

Swapping one token for the other adjusts reserves while keeping the product constant. Slippage increases with trade size relative to pool depth.

## Rewards

`liquidity_reward.rs` distributes epoch-based incentives proportional to provider shares.

## Persistence

Pools are stored under RocksDB keys prefixed `amm/` so nodes can restore state on restart.

## Metrics

- `amm_swap_total` counts successful swaps.
- `liquidity_rewards_disbursed_total` tracks distributed rewards.
- Pool reserves and reward APY appear in explorer charts under `#/dex/pools`,
  and providers should monitor impermanent loss when supplying liquidity.