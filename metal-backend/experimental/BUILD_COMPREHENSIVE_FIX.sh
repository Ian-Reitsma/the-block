#!/bin/bash
# Build script for comprehensive Metal FlashAttention backward support
# This builds Orchard with full MTLBuffer strategy for both shared and private storage
#
# What was changed:
# 1. flash_attn.mm: Added comprehensive MTLBuffer retrieval strategy
#    - Strategy 1: Try shared storage path (public IMPSAllocator interface)
#    - Strategy 2: Attempt private storage path (internal MPSHeapAllocatorImpl)
#    - Both paths handled with clear error messages and fallbacks
#
# 2. orchard_bridge/flash_attn_function.py: Updated backward pass
#    - Pre-allocate grad tensors with shared storage hints
#    - Ensure all inputs use shared storage via _ensure_shared_mps_tensor()
#    - Comprehensive error handling with graceful fallback to reference implementation
#
# Result: Metal backward kernel now attempts to run on any tensor allocation,
# with automatic fallback to PyTorch reference if Metal constraints can't be met.

set -e

echo "======================================="
echo "Building Orchard with Comprehensive Fix"
echo "======================================="
echo ""

CD_PATH="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Working directory: $CD_PATH"
echo ""

echo "Step 1: Cleaning previous builds..."
if [ -d "$CD_PATH/orchard_ops/build" ]; then
  rm -rf "$CD_PATH/orchard_ops/build"
  echo "  ✓ Cleaned build directory"
else
  echo "  ℹ No previous build found"
fi
echo ""

echo "Step 2: Creating build directory..."
mkdir -p "$CD_PATH/orchard_ops/build"
echo "  ✓ Created $CD_PATH/orchard_ops/build"
echo ""

echo "Step 3: Running CMake configuration..."
cd "$CD_PATH/orchard_ops/build"
cmake .. -DCMAKE_BUILD_TYPE=Release
echo "  ✓ CMake configuration complete"
echo ""

echo "Step 4: Building with make (parallel jobs)..."
make -j$(sysctl -n hw.ncpu)
echo "  ✓ Build complete"
echo ""

echo "Step 5: Verifying dylib creation..."
if [ -f "$CD_PATH/orchard_ops/build/libflash_attn.dylib" ]; then
  echo "  ✓ libflash_attn.dylib created successfully"
  echo "  Location: $CD_PATH/orchard_ops/build/libflash_attn.dylib"
  ls -lh "$CD_PATH/orchard_ops/build/libflash_attn.dylib"
else
  echo "  ✗ ERROR: libflash_attn.dylib not found!"
  exit 1
fi
echo ""

echo "Step 6: Running smoke test..."
cd "$CD_PATH/../../"
echo "  Running: ORCHARD_DEBUG_FLASHATN=1 python3 -m pytest -q test_mps_smoke_training.py -s"
echo ""
ORCHARD_DEBUG_FLASHATN=1 python3 -m pytest -q test_mps_smoke_training.py -s

echo ""
echo "======================================="
echo "✓ Build and test complete!"
echo "======================================="
echo ""
echo "Summary of comprehensive fix:"
echo "  1. flash_attn.mm: Dual-strategy MTLBuffer retrieval"
echo "     - Shared storage path (public API)"
echo "     - Private storage path (internal allocator)"
echo "  2. flash_attn_function.py: Robust backward pass"
echo "     - Pre-allocate grads with shared hints"
echo "     - Ensure inputs use shared storage"
echo "     - Graceful fallback to PyTorch reference"
echo ""
echo "Metal backward will now:"
echo "  ✓ Attempt native Metal kernel first"
echo "  ✓ Handle both shared and private tensor allocation"
echo "  ✓ Fall back automatically if constraints not met"
echo "  ✓ Training always succeeds (with fallback as needed)"
echo ""
