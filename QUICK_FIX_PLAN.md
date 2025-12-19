# Quick Fix Plan: Critical Issues

**Goal**: Make code compilable and CI-ready in next 90 minutes  
**Priority**: Critical → High → Medium  

---

## 🔴 Phase 1: Critical Compilation Fixes (1 hour)

### Fix #1: Telemetry Imports (30 minutes)

**Files**: `node/src/telemetry/treasury.rs`, `node/src/telemetry/energy.rs`

**Changes Required**:
```rust
// OLD:
use foundation_metrics::{describe_counter, describe_gauge, describe_histogram, Counter, Gauge, Histogram};
use std::sync::LazyLock;

// NEW:
use foundation_telemetry::{Counter, Gauge, Histogram, IntGauge, Register};
use concurrency::Lazy;

// OLD:
static GOVERNANCE_DISBURSEMENTS_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    describe_counter!("governance_disbursements_total", "...")
});

// NEW:
#[cfg(feature = "telemetry")]
pub static GOVERNANCE_DISBURSEMENTS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    foundation_telemetry::register_counter!(
        "governance_disbursements_total",
        "Total number of treasury disbursements by final status"
    )
    .unwrap_or_else(|_| Counter::placeholder())
});

// Add feature gates to all public functions:
#[cfg(feature = "telemetry")]
pub fn increment_disbursements(status: &str) {
    GOVERNANCE_DISBURSEMENTS_TOTAL
        .with_label_values(&[status])
        .inc();
}

#[cfg(not(feature = "telemetry"))]
pub fn increment_disbursements(_status: &str) {}
```

**Action**: I'll create corrected versions as `treasury_fixed.rs` and `energy_fixed.rs`

### Fix #2: Module Declarations (5 minutes)

**Check and Update**:
1. `governance/src/lib.rs` → add `pub mod treasury_deps;`
2. `node/src/telemetry/mod.rs` or `node/src/lib.rs` → declare treasury/energy modules

**Action**: Check existing module structure first

### Fix #3: Integration Test Paths (20 minutes)

**File**: `tests/integration/treasury_lifecycle_test.rs`

**Likely Changes**:
```rust
// Check actual module paths in node/src/lib.rs
// May need:
use the_block::governance::treasury_deps::*;
use the_block::node::treasury_executor::*;
use the_block::node::telemetry::treasury;

// Add test setup
#[tokio::test]
async fn test_disbursement_lifecycle() {
    // Initialize test harness
    let (state, consensus) = setup_test_environment().await;
    // ...
}
```

### Fix #4: Error Type Integration (20 minutes)

**File**: `governance/src/treasury_deps.rs`

**Action**: 
1. Check if `governance` already has error types
2. Integrate with `thiserror` if used
3. Ensure `DependencyError` converts to parent error type

---

## 🟠 Phase 2: CI and Operations (1.5 hours)

### Fix #5: CI Workflow (15 minutes)

**File**: `.github/workflows/fast-mainnet.yml`

**Changes**:
```yaml
# Fix runner
runs-on: ubuntu-latest  # not ubuntu-latest-m

# Verify commands exist
- name: Lint
  run: |
    # Check if Justfile exists
    if [ -f Justfile ]; then
      just lint
    else
      cargo clippy --all-targets -- -D warnings
    fi
```

### Fix #6: CLI Commands in Runbooks (1 hour)

**File**: `docs/operations.md`

**Action**:
1. Check actual CLI name: `ls bin/` or check `cli/src/main.rs`
2. Test each command:
   ```bash
   ./target/release/the-block-cli --help
   ./target/release/the-block-cli gov --help
   ./target/release/the-block-cli gov treasury --help
   ```
3. Replace all 80+ placeholder commands

### Fix #7: Prometheus Query Helper (15 minutes)

**File**: `docs/operations.md`

**Add to beginning**:
```bash
# Helper function for Prometheus queries
prometheus_query() {
  local query="$1"
  local prometheus_url="${PROMETHEUS_URL:-http://localhost:9090}"
  curl -s "${prometheus_url}/api/v1/query" \
    --data-urlencode "query=${query}" | \
    jq -r '.data.result[0].value[1] // "no data"'
}
```

---

## 🟡 Phase 3: Dashboard and Monitoring (30 minutes)

### Fix #8: Grafana Datasource UIDs (10 minutes)

**Files**: Both dashboard JSONs

**Options**:
1. Use variable: `"uid": "${DS_PROMETHEUS}"`
2. Or document: "Replace 'prometheus' with your datasource UID"
3. Or use templating: `"uid": "[[DS_PROMETHEUS]]"`

### Fix #9: Documentation Updates (20 minutes)

1. Alert threshold tuning guidance
2. Prometheus URL configuration
3. Placeholder URL notes
4. SSH setup requirements

---

## Immediate Actions (Next 10 Minutes)

1. ✅ Create this fix plan
2. ✅ Create audit report
3. ⏳ Check existing code structure:
   ```bash
   # Module structure
   grep "pub mod" governance/src/lib.rs
   grep "pub mod" node/src/lib.rs
   
   # CLI structure
   ls -la cli/src/
   
   # Existing telemetry pattern
   head -50 node/src/telemetry/receipts.rs
   ```
4. ⏳ Create corrected telemetry files
5. ⏳ Test compilation

---

## Success Criteria

### Phase 1 Complete:
- [ ] `cargo build --all-features` succeeds
- [ ] `cargo test --lib` succeeds
- [ ] No compilation errors

### Phase 2 Complete:
- [ ] CI workflow runs locally (with `act` or manual)
- [ ] Runbook commands return valid output
- [ ] All helper functions defined

### Phase 3 Complete:
- [ ] Dashboards import successfully
- [ ] Panels show "Loading..." not "Error"
- [ ] Documentation has no broken links

---

## Testing Script

```bash
#!/bin/bash
set -e

echo "=== Phase 1: Compilation ==="
cargo build --all-features 2>&1 | tail -20
cargo test --lib 2>&1 | tail -20

echo "=== Phase 2: CI ==="
# Test CI steps manually
cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
cargo test -p governance_spec 2>&1 | tail -10

echo "=== Phase 3: Dashboards ==="
# Validate JSON
for f in monitoring/grafana_*.json; do
  echo "Validating $f"
  jq empty "$f" && echo "  ✓ Valid JSON"
done

echo "=== All Phases Complete ==="
```

---

**Start Time**: Now  
**Target Completion**: 90 minutes  
**Next Review**: After Phase 1 (60 minutes)  
