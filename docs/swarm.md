# Local Swarm
> **Review (2025-09-25):** Synced Local Swarm guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

`just swarm-up` launches a 5-node test network with the foundation telemetry dashboard generator.

```sh
just swarm-up
```

To stop and archive logs:

```sh
just swarm-down
```

Follow logs live:

```sh
just swarm-logs
```

`just swarm-test` boots the swarm, runs `net_gossip` tests against it, and tears it down.

Ports start at 35001 for RPC and increment. Override with `SWARM_BASE` env.
Enable chaos mode to randomly restart nodes:

```sh
sh scripts/swarm.sh chaos
```
