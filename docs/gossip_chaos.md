# Gossip Chaos Harness

The chaos harness exercises gossip under adverse conditions by randomly
dropping 10–15% of messages and injecting 50–200 ms of jitter. The test asserts
that the orphan rate remains below 8% and convergence occurs within three
ticks.

Run it via `cargo test --test gossip_chaos`.
