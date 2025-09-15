# DEX AMM Pools

This module implements constant-product automated market maker (AMM) pools for CT/IT pairs. Liquidity providers deposit both tokens and receive pool shares representing their claim on the reserves. Swaps maintain the invariant `x * y = k`.

## Liquidity

- **Add liquidity**: deposit CT and IT in proportion to current reserves to mint shares.
- **Remove liquidity**: burn shares to withdraw proportional reserves.
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
