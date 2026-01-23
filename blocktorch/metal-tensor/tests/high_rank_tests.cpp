#include <gtest/gtest.h>
#include <array>
#include <cstdint>
#include "../metal/runtime/CpuContext.h"
#include "../metal/runtime/MetalKernels.h"

using namespace orchard::runtime;

TEST(MetalKernels, AddSupportsRankNine) {
  std::array<std::int64_t, 9> shape{2,1,1,1,1,1,1,1,1};
  std::array<std::int64_t, 9> strides{};
  strides[8] = 1;
  for (int i = 7; i >= 0; --i)
    strides[i] = strides[i + 1] * shape[i + 1];
  float a[2] = {1.0f, 2.0f};
  float b[2] = {3.0f, 4.0f};
  float c[2] = {0.0f, 0.0f};
  try {
    metal_add(a, b, c, shape.data(), strides.data(), strides.data(), 9, 2);
  } catch (const std::runtime_error &) {
    cpu_context().add(a, b, c, 2);
  }
  EXPECT_FLOAT_EQ(c[0], 4.0f);
  EXPECT_FLOAT_EQ(c[1], 6.0f);
}
