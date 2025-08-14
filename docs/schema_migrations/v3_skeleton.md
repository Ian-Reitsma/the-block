# Schema Migration to Version 3

This placeholder outlines the required steps for upgrading an existing node to schema_version = 3.

1. Scan all accounts and initialize `pending_consumer`, `pending_industrial`, and `pending_nonce` to zero.
2. Walk the mempool bucket and accumulate per-account reservations.
3. Write the new state to a shadow database, then atomically swap directories.

Implementation details will be provided in the final migration tool.
