import numpy as np
import pandas as pd
import time
import sys

# --- Hard constraints ---
usd_total = 1000
n_pools = 4
usd_per_pool = usd_total / n_pools
sim_days = 30
blocks_per_day = 86_400  # 1‑second blocks

# --- Search space (adjust for debug vs prod) ---
genesis_range = range(400_000, 1_600_000, 100_000)  # tokens minted at genesis
founder_pct_range = np.arange(0.003, 0.013, 0.001)  # 0.3 % – 1.2 %
pool_pct_range = np.arange(0.30, 0.56, 0.03)  # 30 % – 55 % to DEX pools
block_reward_range = np.arange(0.006, 0.031, 0.001)  # 0.006 – 0.03 / sec
decay_range = np.arange(0.99995, 0.999986, 0.000006)  # 0.99995 – 0.999986


def simulate_mining(reward, decay, days, blocks=blocks_per_day):
    mined = 0.0
    r = reward
    for _ in range(days * blocks):
        mined += r
        r *= decay
    return mined


def amm_start_price(tokens_in_pool, usd_liq):
    return usd_liq / tokens_in_pool


def fitness(
    genesis, founder_pct, pool_pct, mined_30d, inflation, start_price, founder_ratio
):
    reserve_pct = 1 - founder_pct - pool_pct * 1.0
    price_target = 0.0025
    score = (
        (1 - founder_pct / 0.012) * 10
        + (1 - abs(pool_pct - 0.45) / 0.15) * 8
        + (reserve_pct) * 6
        + (1 - abs(founder_ratio - 1) / 0.2) * 10
        + (1 - inflation / 0.06) * 8
        + (1 - abs(start_price - price_target) / price_target) * 8
    )
    return score


# --- Progress setup ---
param_grid = (
    len(genesis_range)
    * len(founder_pct_range)
    * len(pool_pct_range)
    * len(block_reward_range)
    * len(decay_range)
)
print(
    f"Launching parameter sweep: {param_grid:,} scenarios. This may take 1–3 min on a laptop."
)
records = []
record_counter = 0
best = None
best_score = -1e9

progress_interval = 10.0  # seconds between progress updates
next_progress = time.time() + progress_interval
start_time = time.time()


def print_progress(completed, total, elapsed):
    pct = completed / total * 100
    eta = (elapsed / completed) * (total - completed) if completed else 0
    bar_len = 36
    bar = "#" * int(bar_len * pct / 100) + "-" * (bar_len - int(bar_len * pct / 100))
    sys.stdout.write(
        f"\r[{bar}] {pct:5.1f}% ({completed:,}/{total:,})  "
        f"{elapsed:5.1f}s elapsed, ETA {eta/60:4.1f} min  "
    )
    sys.stdout.flush()


for g_idx, genesis in enumerate(genesis_range):
    for f_idx, founder_pct in enumerate(founder_pct_range):
        founder_tokens = int(genesis * founder_pct)
        for p_idx, pool_pct in enumerate(pool_pct_range):
            tokens_per_pool = int((genesis * pool_pct) / n_pools)
            if tokens_per_pool == 0:
                continue
            start_price = amm_start_price(tokens_per_pool, usd_per_pool)
            if not (0.001 <= start_price <= 0.0045):
                continue
            for br_idx, reward in enumerate(block_reward_range):
                for d_idx, decay in enumerate(decay_range):
                    mined_30d = simulate_mining(reward, decay, sim_days)
                    inflation_30d = mined_30d / genesis
                    if inflation_30d > 0.06:
                        continue
                    founder_ratio = (
                        mined_30d / founder_tokens if founder_tokens else np.inf
                    )
                    if not (0.8 <= founder_ratio <= 1.2):
                        continue
                    score = fitness(
                        genesis,
                        founder_pct,
                        pool_pct,
                        mined_30d,
                        inflation_30d,
                        start_price,
                        founder_ratio,
                    )
                    records.append(
                        (
                            genesis,
                            founder_pct,
                            pool_pct,
                            reward,
                            decay,
                            mined_30d,
                            inflation_30d,
                            start_price,
                            founder_ratio,
                            score,
                        )
                    )
                    record_counter += 1
                    now = time.time()
                    if now > next_progress:
                        print_progress(record_counter, param_grid, now - start_time)
                        next_progress = now + progress_interval
                    if score > best_score:
                        best_score = score
                        best = records[-1]

# Print final progress bar
print_progress(record_counter, param_grid, time.time() - start_time)
print("\nSweep complete.")

# --- Results formatting ---
cols = [
    "genesis",
    "founder_pct",
    "pool_pct",
    "block_reward",
    "decay",
    "mined_30d",
    "inflation_30d",
    "start_price",
    "founder_ratio",
    "score",
]
df = pd.DataFrame(records, columns=cols)
df_top = df.sort_values("score", ascending=False).head(10)


def format_row(row):
    return {
        "genesis": int(row["genesis"]),
        "founder_tokens": int(row["genesis"] * row["founder_pct"]),
        "founder_pct": f"{row['founder_pct']*100:.2f}%",
        "pool_pct": f"{row['pool_pct']*100:.1f}%",
        "block_reward": f"{row['block_reward']:.5f}/sec",
        "decay": f"{row['decay']:.5f}",
        "mined_30d": f"{row['mined_30d']:.0f}",
        "inflation_30d": f"{row['inflation_30d']*100:.2f}%",
        "start_price": f"${row['start_price']:.5f}",
        "founder_ratio": f"{row['founder_ratio']:.2f}×",
    }


print("\n===== Optimized Launch Parameters (Best) =====")
print(pd.Series(format_row(df_top.iloc[0])))

print("\n===== Top 10 Scenarios (Best to Worst) =====")
for i, row in df_top.iterrows():
    print(f"\nScenario {i+1}:")
    for k, v in format_row(row).items():
        print(f"  {k:15s}: {v}")

print("\nDone.")

# --- Debug: To shrink run time, reduce search ranges above ---
# genesis_range = range(600_000, 900_000, 100_000)
# founder_pct_range = np.arange(0.004, 0.007, 0.001)
# pool_pct_range = np.arange(0.40, 0.48, 0.03)
# block_reward_range = np.arange(0.008, 0.013, 0.001)
# decay_range = np.arange(0.99996, 0.99998, 0.00001)
