#!/bin/bash
# Receipt Integration Verification Script
# Run this after completing BlockEncoder manual updates

set -e  # Exit on error

echo "====================================="
echo "Receipt Integration Verification"
echo "====================================="
echo ""

cd "$(dirname "$0")"

ERRORS=0

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

function check_step() {
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓${NC} $1"
    else
        echo -e "${RED}✗${NC} $1"
        ERRORS=$((ERRORS + 1))
    fi
}

echo "Step 1: Check for BlockEncoder usage"
echo "--------------------------------------"
BLOCKENCODER_COUNT=$(grep -r "BlockEncoder {" node/src/ 2>/dev/null | wc -l || echo "0")
echo "Found $BLOCKENCODER_COUNT BlockEncoder instantiation(s)"

if [ "$BLOCKENCODER_COUNT" -gt 0 ]; then
    echo -e "${YELLOW}Manual verification needed:${NC}"
    grep -rn "BlockEncoder {" node/src/ 2>/dev/null || true
    echo ""
    echo "Verify each instantiation includes 'receipts_serialized' field"
else
    echo "No BlockEncoder instantiations found (or already updated)"
fi
echo ""

echo "Step 2: Compile library"
echo "--------------------------------------"
cargo build --lib 2>&1 | tail -20
check_step "Library compilation"
echo ""

echo "Step 3: Run unit tests"
echo "--------------------------------------"

echo "  3a: Receipt tests..."
cargo test --lib receipts 2>&1 | tail -10
check_step "Receipt unit tests"

echo "  3b: Block binary tests..."
cargo test --lib block_binary 2>&1 | tail -10
check_step "Block binary tests"

echo "  3c: Hash tests..."
cargo test --lib hash 2>&1 | tail -10
check_step "Hash tests"

echo "  3d: Deterministic metrics tests..."
cargo test --lib deterministic_metrics 2>&1 | tail -10
check_step "Deterministic metrics tests"
echo ""

echo "Step 4: Run integration tests"
echo "--------------------------------------"
cargo test --test receipt_integration 2>&1 | tail -15
check_step "Receipt integration tests"
echo ""

echo "Step 5: Check telemetry module"
echo "--------------------------------------"
if grep -q "pub mod receipts" node/src/telemetry.rs; then
    echo -e "${GREEN}✓${NC} Receipts module exported in telemetry.rs"
else
    echo -e "${RED}✗${NC} Receipts module NOT exported in telemetry.rs"
    ERRORS=$((ERRORS + 1))
fi
echo ""

echo "Step 6: Verify critical files exist"
echo "--------------------------------------"
FILES_TO_CHECK=(
    "node/src/receipts.rs"
    "node/src/telemetry/receipts.rs"
    "node/src/economics/deterministic_metrics.rs"
    "node/tests/receipt_integration.rs"
    "docs/archive/RECEIPT_INTEGRATION_COMPLETE.md"
    "MARKET_RECEIPT_INTEGRATION.md"
)

for file in "${FILES_TO_CHECK[@]}"; do
    if [ -f "$file" ]; then
        echo -e "${GREEN}✓${NC} $file"
    else
        echo -e "${RED}✗${NC} $file MISSING"
        ERRORS=$((ERRORS + 1))
    fi
done
echo ""

echo "Step 7: Check for encode_receipts function"
echo "--------------------------------------"
if grep -q "pub fn encode_receipts" node/src/block_binary.rs; then
    echo -e "${GREEN}✓${NC} encode_receipts() function exists"
else
    echo -e "${RED}✗${NC} encode_receipts() function NOT found"
    ERRORS=$((ERRORS + 1))
fi
echo ""

echo "Step 8: Check BlockEncoder struct"
echo "--------------------------------------"
if grep -q "receipts_serialized" node/src/hashlayout.rs; then
    echo -e "${GREEN}✓${NC} receipts_serialized field in BlockEncoder"
else
    echo -e "${RED}✗${NC} receipts_serialized field NOT in BlockEncoder"
    ERRORS=$((ERRORS + 1))
fi

if grep -q "h.update(self.receipts_serialized)" node/src/hashlayout.rs; then
    echo -e "${GREEN}✓${NC} receipts included in hash calculation"
else
    echo -e "${RED}✗${NC} receipts NOT included in hash calculation"
    ERRORS=$((ERRORS + 1))
fi
echo ""

echo "====================================="
echo "Verification Summary"
echo "====================================="

if [ $ERRORS -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo ""
    echo "Next steps:"
    echo "1. If BlockEncoder instantiations found, verify they include receipts_serialized"
    echo "2. Start market integration (see MARKET_RECEIPT_INTEGRATION.md)"
    echo "3. Deploy to testnet"
    exit 0
else
    echo -e "${RED}✗ $ERRORS error(s) found${NC}"
    echo ""
    echo "Please fix the errors above before proceeding."
    echo "See docs/archive/RECEIPT_INTEGRATION_COMPLETE.md for details."
    exit 1
fi
