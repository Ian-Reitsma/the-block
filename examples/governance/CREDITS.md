# Credits CLI Examples

Demonstrate basic usage of the node's `credits` subcommands and governance-driven issuance.

Check a provider balance:

```bash
cargo run --bin node -- credits balance alice
```

To mint credits, craft and execute a governance proposal:

```bash
# submit a credit issuance proposal
cargo run --example gov -- submit examples/governance/issue_credits.json
# vote from both houses (ids start at 0)
cargo run --example gov -- vote 0 --house ops
cargo run --example gov -- vote 0 --house builders
# execute after quorum and timelock, applying credits to the ledger
cargo run --example gov -- exec 0 --data-dir node-data
```

The provider's new balance can then be inspected with the `credits balance` command.
