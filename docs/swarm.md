# Local Swarm
> **Review (2025-09-24):** Validated for the dependency-sovereignty pivot; third-token references removed; align changes with the in-house roadmap.

`just swarm-up` launches a 5-node test network with Prometheus and Grafana.

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