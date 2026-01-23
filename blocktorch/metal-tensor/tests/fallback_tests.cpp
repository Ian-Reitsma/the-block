#include "core/autograd/Node.h"
#include "core/tensor/Tensor.h"
#include "runtime/MetalContext.h"
#include "runtime/Runtime.h"
#include <array>
#include <cstdlib>
#include <gtest/gtest.h>

using namespace orchard::core::tensor;
using namespace orchard::core::autograd;

struct AccNode : Node {
  using Node::accumulate;
  void apply(Tensor &) override {}
};

TEST(FallbackTest, AccumulateFallsBackToCpu) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::mps);
  t.set_requires_grad(true);
  Tensor g = Tensor::empty(shape, DType::f32, Device::mps);
  auto *gptr = static_cast<float *>(g.data_ptr());
  gptr[0] = 1.0f;
  gptr[1] = 1.0f;
  AccNode::accumulate(t, g);
  auto *grad = static_cast<float *>(t.grad().data_ptr());
  EXPECT_FLOAT_EQ(grad[0], 1.0f);
  EXPECT_FLOAT_EQ(grad[1], 1.0f);
}
TEST(FallbackTest, CopyBuffersThrowsWithoutDevice) {
  if (orchard::runtime::metal_context().has_device())
    GTEST_SKIP() << "Metal device present; skipping CPU fallback test.";
  std::array<std::int64_t, 8> shape{1, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  EXPECT_THROW(
      {
        try {
          orchard::runtime::metal_copy_buffers(
              static_cast<orchard::runtime::MTLBufferRef>(a.data_ptr()),
              static_cast<orchard::runtime::MTLBufferRef>(b.data_ptr()),
              sizeof(float));
        } catch (const std::runtime_error &e) {
          EXPECT_STREQ("Metal device unavailable", e.what());
          throw;
        }
      },
      std::runtime_error);
}

TEST(FallbackTest, AddFallsBackWhenKernelMissing) {
  const char *orig = std::getenv("ORCHARD_KERNEL_DIR");
  setenv("ORCHARD_KERNEL_DIR", "/tmp/orchard_missing", 1);
  std::array<std::int64_t, 8> shape{1, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::mps);
  Tensor b = Tensor::empty(shape, DType::f32, Device::mps);
  a.fill(1.0f);
  b.fill(1.0f);
  Tensor out;
  EXPECT_NO_THROW({ out = a.add(b); });
  auto *ptr = static_cast<float *>(out.data_ptr());
  EXPECT_FLOAT_EQ(ptr[0], 2.0f);
  if (orig)
    setenv("ORCHARD_KERNEL_DIR", orig, 1);
  else
    unsetenv("ORCHARD_KERNEL_DIR");
}
