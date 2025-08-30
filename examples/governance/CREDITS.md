# Credits CLI Examples

Demonstrate basic usage of the node's `credits` subcommands.

Top up a provider and view balance:

```bash
cargo run --bin node -- credits top-up --provider alice --amount 50
cargo run --bin node -- credits balance alice
```

`top-up` is a development convenience; production credit issuance is governed on-chain.
