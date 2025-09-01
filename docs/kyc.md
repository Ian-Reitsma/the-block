# Optional KYC Hooks

The node exposes an optional Know-Your-Customer (KYC) verification flow for
businesses that must vet participants. Verification is entirely off-chain and
pluggable so the default build incurs no dependency or requirement.

## Plugin Interface

```rust
use the_block::kyc::{set_provider, KycProvider};

struct MyProvider;
impl KycProvider for MyProvider {
    fn verify(&self, user: &str) -> Result<bool, String> {
        // call out to third-party service
        Ok(user == "trusted")
    }
}

set_provider(Arc::new(MyProvider));
```

## RPC Trigger

Clients may request verification via:

```json
{"method": "kyc.verify", "params": {"user": "alice"}}
```

The default provider always returns `"verified"`, preserving the current
bypass semantics when no plugin is installed.

## On-Chain Impact

KYC results are not recorded on-chain and have no effect on consensus or
transaction processing.
