#include <gtest/gtest.h>

#include <array>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <stdexcept>
#include <string>

#include "common/Profiling.h"
#include "core/tensor/Debug.h"
#include "core/tensor/Tensor.h"
#include "runtime/Allocator.h"
#include "runtime/CpuContext.h"
#include "runtime/MetalContext.h"

using namespace orchard::core::tensor;

TEST(TensorTest, ToCpuZeroCopy) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor cpu = t.to(Device::cpu);
  EXPECT_EQ(t.data_ptr(), cpu.data_ptr());
}

TEST(TensorTest, ViewSliceMutation) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 4; ++i)
    base[i] = static_cast<float>(i);

  std::array<std::int64_t, 8> newShape{2, 2, 1, 1, 1, 1, 1, 1};
  Tensor v = t.view(newShape);
  auto *vptr = static_cast<float *>(v.data_ptr());
  vptr[1] = 42.0f;
  EXPECT_EQ(base[1], 42.0f);

  Tensor s = t.slice(0, 0, 2);
  auto *sptr = static_cast<float *>(s.data_ptr());
  sptr[1] = 99.0f;
  EXPECT_EQ(base[1], 99.0f);
}

TEST(TensorTest, ViewInvalidShape) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  std::array<std::int64_t, 8> badShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor v = t.view(badShape);
  EXPECT_EQ(v.data_ptr(), nullptr);
}

TEST(TensorTest, SliceOffsetStart) {
  std::array<std::int64_t, 8> shape{5, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 5; ++i)
    base[i] = static_cast<float>(i);
  Tensor s = t.slice(0, 2, 5);
  auto *sptr = static_cast<float *>(s.data_ptr());
  EXPECT_EQ(sptr[0], base[2]);
  EXPECT_EQ(s.offset(), 2);
}

TEST(TensorTest, CloneDistinctStorage) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(t.data_ptr());
  base[0] = 1.0f;
  base[1] = 2.0f;
  Tensor c = t.clone();
  EXPECT_NE(c.data_ptr(), t.data_ptr());
  auto *cptr = static_cast<float *>(c.data_ptr());
  EXPECT_EQ(cptr[0], base[0]);
  base[0] = 3.0f;
  EXPECT_NE(cptr[0], base[0]);
}

TEST(TensorTest, DetachSharesStorage) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(t.data_ptr());
  base[0] = 1.0f;
  Tensor d = t.detach();
  EXPECT_TRUE(d.is_alias_of(t));
  auto *dptr = static_cast<float *>(d.data_ptr());
  dptr[0] = 5.0f;
  EXPECT_FLOAT_EQ(base[0], 5.0f);
}

TEST(TensorTest, CloneBeforeDetachIndepStorage) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(t.data_ptr());
  base[0] = 1.0f;
  Tensor d = t.clone().detach();
  EXPECT_FALSE(d.is_alias_of(t));
  auto *dptr = static_cast<float *>(d.data_ptr());
  dptr[0] = 7.0f;
  EXPECT_FLOAT_EQ(base[0], 1.0f);
}

#ifdef __APPLE__
TEST(TensorTest, DetachSharesStorageMps) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(cpu.data_ptr());
  base[0] = 8.0f;
  Tensor metal = cpu.to(Device::mps);
  Tensor d = metal.detach();
  EXPECT_TRUE(d.is_alias_of(metal));
  d.div_(2.0f);
  Tensor back = metal.to(Device::cpu);
  auto *bptr = static_cast<float *>(back.data_ptr());
  EXPECT_FLOAT_EQ(bptr[0], 4.0f);
}

TEST(TensorTest, CloneBeforeDetachIndepStorageMps) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(cpu.data_ptr());
  base[0] = 6.0f;
  Tensor metal = cpu.to(Device::mps);
  Tensor clone = metal.clone().detach();
  EXPECT_FALSE(clone.is_alias_of(metal));
  clone.div_(2.0f);
  Tensor back = metal.to(Device::cpu);
  auto *bptr = static_cast<float *>(back.data_ptr());
  EXPECT_FLOAT_EQ(bptr[0], 6.0f);
}
#endif

TEST(TensorTest, FromDataZeroCopyAndDeleter) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  void *raw = nullptr;
  posix_memalign(&raw, 64, 2 * sizeof(float));
  auto *src = static_cast<float *>(raw);
  src[0] = 1.0f;
  src[1] = 2.0f;
  bool freed = false;
  {
    Tensor t =
        Tensor::fromData(src, shape, DType::f32, Device::cpu, [&](void *p) {
          free(p);
          freed = true;
        });
    auto *ptr = static_cast<float *>(t.data_ptr());
    EXPECT_EQ(ptr, src);
    src[0] = 3.0f;
    EXPECT_EQ(ptr[0], 3.0f);
  }
  EXPECT_TRUE(freed);
}

TEST(TensorTest, DataPtrAlignment) {
  std::array<std::int64_t, 8> shape{1, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  std::uintptr_t addr = reinterpret_cast<std::uintptr_t>(t.data_ptr());
  EXPECT_EQ(addr % 64, 0u);
}

TEST(TensorAutogradTest, AddBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i);
    bp[i] = static_cast<float>(i * 2);
  }
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.add(b);
  c.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], 1.0f);
    EXPECT_FLOAT_EQ(bg[i], 1.0f);
  }
}

TEST(TensorAutogradTest, MulBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i + 2);
  }
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.mul(b);
  c.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], bp[i]);
    EXPECT_FLOAT_EQ(bg[i], ap[i]);
  }
}

TEST(TensorAutogradTest, DivBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 2);
    bp[i] = static_cast<float>(i + 1);
  }
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.div(b);
  c.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], 1.0f / bp[i]);
    EXPECT_FLOAT_EQ(bg[i], -ap[i] / (bp[i] * bp[i]));
  }
}

#ifndef __APPLE__
TEST(TensorAutogradTest, DivBackwardSafeCpu) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  ap[0] = 1.0f;
  ap[1] = 2.0f;
  ap[2] = 3.0f;
  bp[0] = 1.0f;
  bp[1] = 0.0f;
  bp[2] = 2.0f;
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.div(b, true);
  c.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  EXPECT_FLOAT_EQ(ag[0], 1.0f / bp[0]);
  EXPECT_FLOAT_EQ(ag[1], 0.0f);
  EXPECT_FLOAT_EQ(ag[2], 1.0f / bp[2]);
  EXPECT_FLOAT_EQ(bg[0], -ap[0] / (bp[0] * bp[0]));
  EXPECT_FLOAT_EQ(bg[1], 0.0f);
  EXPECT_FLOAT_EQ(bg[2], -ap[2] / (bp[2] * bp[2]));
}

TEST(TensorAutogradTest, AddBackwardCpu) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i);
    bp[i] = static_cast<float>(i * 2);
  }
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.add(b);
  c.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], 1.0f);
    EXPECT_FLOAT_EQ(bg[i], 1.0f);
  }
}

TEST(TensorAutogradTest, MulBackwardCpu) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i + 2);
  }
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.mul(b);
  c.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], bp[i]);
    EXPECT_FLOAT_EQ(bg[i], ap[i]);
  }
}

TEST(TensorAutogradTest, TransposeBackwardCpu) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> cShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor c = Tensor::empty(cShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *cp = static_cast<float *>(c.data_ptr());
  for (int i = 0; i < 6; ++i) {
    ap[i] = static_cast<float>(i + 1);
    cp[i] = static_cast<float>(i + 1);
  }
  a.set_requires_grad(true);
  c.set_requires_grad(true);
  Tensor t = a.transpose(0, 1);
  Tensor prod = t.mul(c);
  Tensor s = prod.sum();
  s.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 2; ++i) {
    for (int j = 0; j < 3; ++j) {
      EXPECT_FLOAT_EQ(ag[i * 3 + j], cp[j * 2 + i]);
    }
  }
  auto *cg = static_cast<float *>(c.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    for (int j = 0; j < 2; ++j) {
      EXPECT_FLOAT_EQ(cg[i * 2 + j], ap[j * 3 + i]);
    }
  }
}

TEST(TensorAutogradTest, TransposeBackwardDoubleCpu) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> cShape{2, 3, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor c = Tensor::empty(cShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *cp = static_cast<float *>(c.data_ptr());
  for (int i = 0; i < 6; ++i) {
    ap[i] = static_cast<float>(i + 1);
    cp[i] = static_cast<float>(i + 7);
  }
  a.set_requires_grad(true);
  c.set_requires_grad(true);
  Tensor t = a.transpose(0, 1);
  Tensor u = t.transpose(0, 1);
  Tensor prod = u.mul(c);
  Tensor s = prod.sum();
  s.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(ag[i], cp[i]);
  auto *cg = static_cast<float *>(c.grad().data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(cg[i], ap[i]);
}

TEST(TensorAutogradTest, MatmulBackwardCpu) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> bShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(bShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 6; ++i)
    ap[i] = static_cast<float>(i + 1);
  for (int i = 0; i < 6; ++i)
    bp[i] = static_cast<float>(i + 1);
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.matmul(b);
  Tensor s = c.sum();
  s.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  std::array<float, 6> expected_a{3.0f, 7.0f, 11.0f, 3.0f, 7.0f, 11.0f};
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(ag[i], expected_a[i]);
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  std::array<float, 6> expected_b{5.0f, 5.0f, 7.0f, 7.0f, 9.0f, 9.0f};
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(bg[i], expected_b[i]);
}

TEST(TensorAutogradTest, MeanBackwardCpu) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 4; ++i)
    p[i] = static_cast<float>(i + 1);
  t.set_requires_grad(true);
  Tensor m = t.mean();
  m.backward();
  auto *g = static_cast<float *>(t.grad().data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(g[i], 0.25f);
}

TEST(TensorAutogradTest, SumBackwardCpu) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 4; ++i)
    p[i] = static_cast<float>(i + 1);
  t.set_requires_grad(true);
  Tensor s = t.sum();
  s.backward();
  auto *g = static_cast<float *>(t.grad().data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(g[i], 1.0f);
}
#endif

TEST(TensorAutogradTest, DivScalarBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 2);
  a.set_requires_grad(true);
  Tensor b = a.div(2.0f);
  b.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(ag[i], 0.5f);
}

TEST(TensorAutogradTest, DivScalarInplaceBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 2);
  a.set_requires_grad(true);
  a.div_(2.0f);
  a.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(ag[i], 0.5f);
}

TEST(TensorAutogradTest, DivScalarInplaceChainBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor tmp = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *tp = static_cast<float *>(tmp.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    tp[i] = static_cast<float>(i + 1);
  }
  a.set_requires_grad(true);
  tmp.set_requires_grad(true);
  Tensor b = a.add(tmp);
  b.div_(2.0f);
  b.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *tg = static_cast<float *>(tmp.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], 0.5f);
    EXPECT_FLOAT_EQ(tg[i], 0.5f);
  }
}

TEST(TensorAutogradTest, DivScalarDoubleInplaceBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 4);
  a.set_requires_grad(true);
  a.div_(2.0f);
  a.div_(2.0f);
  a.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(ag[i], 0.25f);
}

TEST(TensorAutogradTest, DivScalarChainDoubleInplaceBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor tmp = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *tp = static_cast<float *>(tmp.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    tp[i] = static_cast<float>(i + 1);
  }
  a.set_requires_grad(true);
  tmp.set_requires_grad(true);
  Tensor b = a.add(tmp);
  b.div_(2.0f);
  b.div_(2.0f);
  b.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *tg = static_cast<float *>(tmp.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], 0.25f);
    EXPECT_FLOAT_EQ(tg[i], 0.25f);
  }
}

TEST(TensorAutogradTest, DivScalarTripleInplaceBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 4);
  a.set_requires_grad(true);
  a.div_(2.0f);
  a.div_(2.0f);
  a.div_(2.0f);
  a.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(ag[i], 0.125f);
}

TEST(TensorAutogradTest, DivScalarChainTripleInplaceBackward) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor tmp = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *tp = static_cast<float *>(tmp.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    tp[i] = static_cast<float>(i + 1);
  }
  a.set_requires_grad(true);
  tmp.set_requires_grad(true);
  Tensor b = a.add(tmp);
  b.div_(2.0f);
  b.div_(2.0f);
  b.div_(2.0f);
  b.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  auto *tg = static_cast<float *>(tmp.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(ag[i], 0.125f);
    EXPECT_FLOAT_EQ(tg[i], 0.125f);
  }
}

TEST(TensorAutogradTest, DetachNoGrad) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 1);
  a.set_requires_grad(true);
  Tensor b = a.detach();
  b.set_requires_grad(true);
  Tensor c = b.mul(b);
  c.backward();
  EXPECT_EQ(a.grad().data_ptr(), nullptr);
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(bg[i], 2 * ap[i]);
}

TEST(TensorAutogradTest, TransposeBackward) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> cShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor c = Tensor::empty(cShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *cp = static_cast<float *>(c.data_ptr());
  for (int i = 0; i < 6; ++i) {
    ap[i] = static_cast<float>(i + 1);
    cp[i] = static_cast<float>(i + 1);
  }
  a.set_requires_grad(true);
  c.set_requires_grad(true);
  Tensor t = a.transpose(0, 1);
  Tensor prod = t.mul(c);
  Tensor s = prod.sum();
  s.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 2; ++i) {
    for (int j = 0; j < 3; ++j) {
      EXPECT_FLOAT_EQ(ag[i * 3 + j], cp[j * 2 + i]);
    }
  }
  auto *cg = static_cast<float *>(c.grad().data_ptr());
  for (int i = 0; i < 3; ++i) {
    for (int j = 0; j < 2; ++j) {
      EXPECT_FLOAT_EQ(cg[i * 2 + j], ap[j * 3 + i]);
    }
  }
}

TEST(TensorAutogradTest, MatmulBackward) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> bShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(bShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 6; ++i)
    ap[i] = static_cast<float>(i + 1);
  for (int i = 0; i < 6; ++i)
    bp[i] = static_cast<float>(i + 1);
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor c = a.matmul(b);
  c.backward();
  EXPECT_NE(a.grad().data_ptr(), nullptr);
  EXPECT_NE(b.grad().data_ptr(), nullptr);
}

TEST(TensorAutogradTest, SumMeanBackward) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 4; ++i)
    p[i] = static_cast<float>(i + 1);
  t.set_requires_grad(true);
  Tensor s = t.sum();
  s.backward();
  auto *sg = static_cast<float *>(t.grad().data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(sg[i], 1.0f);
  Tensor t2 = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p2 = static_cast<float *>(t2.data_ptr());
  for (int i = 0; i < 4; ++i)
    p2[i] = static_cast<float>(i + 1);
  t2.set_requires_grad(true);
  Tensor m = t2.mean();
  m.backward();
  auto *mg = static_cast<float *>(t2.grad().data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(mg[i], 0.25f);
}

TEST(TensorAutogradTest, SumMeanAxisBackward) {
  std::array<std::int64_t, 8> shape{2, 3, 4, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 24; ++i)
    p[i] = static_cast<float>(i + 1);
  t.set_requires_grad(true);
  Tensor s = t.sum(1);
  s.backward();
  auto *sg = static_cast<float *>(t.grad().data_ptr());
  for (int i = 0; i < 24; ++i)
    EXPECT_FLOAT_EQ(sg[i], 1.0f);

  Tensor t2 = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p2 = static_cast<float *>(t2.data_ptr());
  for (int i = 0; i < 24; ++i)
    p2[i] = static_cast<float>(i + 1);
  t2.set_requires_grad(true);
  Tensor m = t2.mean(1);
  m.backward();
  auto *mg = static_cast<float *>(t2.grad().data_ptr());
  for (int i = 0; i < 24; ++i)
    EXPECT_FLOAT_EQ(mg[i], 1.0f / 3.0f);
}

TEST(TensorAutogradTest, ViewBackward) {
  std::array<std::int64_t, 8> shape{2, 2, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 4; ++i)
    p[i] = 1.0f;
  t.set_requires_grad(true);
  std::array<std::int64_t, 8> newShape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor v = t.view(newShape);
  Tensor s = v.sum();
  s.backward();
  auto *g = static_cast<float *>(t.grad().data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(g[i], 1.0f);
}

#ifndef __APPLE__
TEST(CpuOnlyAutogradTest, TransposeBackward) {
  std::array<std::int64_t, 8> shape{2, 3, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 6; ++i)
    ap[i] = static_cast<float>(i + 1);
  a.set_requires_grad(true);
  Tensor s = a.transpose(0, 1).sum();
  s.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(ag[i], 1.0f);
}

TEST(CpuOnlyAutogradTest, MatmulBackward) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> bShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(bShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 6; ++i)
    ap[i] = static_cast<float>(i + 1);
  for (int i = 0; i < 6; ++i)
    bp[i] = static_cast<float>(i + 1);
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor s = a.matmul(b).sum();
  s.backward();
  auto *ag = static_cast<float *>(a.grad().data_ptr());
  float expectedA[6] = {3.0f, 7.0f, 11.0f, 3.0f, 7.0f, 11.0f};
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(ag[i], expectedA[i]);
  auto *bg = static_cast<float *>(b.grad().data_ptr());
  float expectedB[6] = {5.0f, 5.0f, 7.0f, 7.0f, 9.0f, 9.0f};
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(bg[i], expectedB[i]);
}

TEST(CpuOnlyAutogradTest, MeanBackward) {
  std::array<std::int64_t, 8> shape{2, 3, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 6; ++i)
    p[i] = static_cast<float>(i + 1);
  t.set_requires_grad(true);
  Tensor m = t.mean();
  m.backward();
  auto *g = static_cast<float *>(t.grad().data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(g[i], 1.0f / 6.0f);
}

TEST(CpuOnlyAutogradTest, SumBackward) {
  std::array<std::int64_t, 8> shape{2, 3, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 6; ++i)
    p[i] = static_cast<float>(i + 1);
  t.set_requires_grad(true);
  Tensor s = t.sum();
  s.backward();
  auto *g = static_cast<float *>(t.grad().data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(g[i], 1.0f);
}
#endif

#ifdef __APPLE__
TEST(TensorTest, CpuMetalRoundtrip) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ptr = static_cast<float *>(cpu.data_ptr());
  for (int i = 0; i < 4; ++i)
    ptr[i] = static_cast<float>(i + 1);
  Tensor metal = cpu.to(Device::mps);
  Tensor back = metal.to(Device::cpu);
  auto *bptr = static_cast<float *>(back.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_EQ(bptr[i], ptr[i]);
}

TEST(TensorTest, CpuMetalRoundtripNonContiguousLarge) {
  const std::int64_t N = 10000;
  std::array<std::int64_t, 8> shape{N, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(cpu.data_ptr());
  for (std::int64_t i = 0; i < N; ++i)
    base[i] = static_cast<float>(i);
  Tensor slice = cpu.slice(0, 0, N, 2);
  EXPECT_FALSE(slice.is_contiguous());
  Tensor metal = slice.to(Device::mps);
  Tensor back = metal.to(Device::cpu);
  auto *bptr = static_cast<float *>(back.data_ptr());
  for (std::int64_t i = 0; i < N / 2; ++i)
    EXPECT_FLOAT_EQ(bptr[i], static_cast<float>(i * 2));
}

TEST(TensorTest, AddMetalMatchesCpu) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i);
    bp[i] = static_cast<float>(i * 2);
  }
  Tensor cpu = a.add(b);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor mc = ma.add(mb).to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mc.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(cp[i], mp[i]);
}

TEST(TensorTest, MulMetalMatchesCpu) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i + 2);
  }
  Tensor cpu = a.mul(b);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor mc = ma.mul(mb).to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mc.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(cp[i], mp[i]);
}

TEST(TensorTest, DivMetalMatchesCpu) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 2);
    bp[i] = static_cast<float>(i + 1);
  }
  Tensor cpu = a.div(b);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor mc = ma.div(mb).to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mc.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(cp[i], mp[i]);
}

TEST(TensorTest, DivScalarMetalMatchesCpu) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 2);
  Tensor cpu = a.div(2.0f);
  Tensor ma = a.to(Device::mps);
  Tensor mc = ma.div(2.0f).to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mc.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(cp[i], mp[i]);
}
#endif

TEST(TensorTest, DivScalarInplace) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    float v = static_cast<float>(i + 2);
    ap[i] = v;
    bp[i] = v;
  }
  a.div_(2.0f);
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(ap[i], static_cast<float>(i + 2) / 2.0f);
#ifdef __APPLE__
  Tensor mb = b.to(Device::mps);
  mb.div_(2.0f);
  Tensor bc = mb.to(Device::cpu);
  auto *bcp = static_cast<float *>(bc.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(bcp[i], static_cast<float>(i + 2) / 2.0f);
#endif
}

#ifdef __APPLE__
TEST(TensorTest, DetachSharesStorageMetal) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(cpu.data_ptr());
  base[0] = 1.0f;
  Tensor m = cpu.to(Device::mps);
  Tensor d = m.detach();
  EXPECT_TRUE(d.is_alias_of(m));
  d.fill(9.0f);
  Tensor back = m.to(Device::cpu);
  auto *bptr = static_cast<float *>(back.data_ptr());
  EXPECT_FLOAT_EQ(bptr[0], 9.0f);
}

TEST(TensorTest, CloneBeforeDetachIndepStorageMetal) {
  std::array<std::int64_t, 8> shape{2, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(cpu.data_ptr());
  base[0] = 1.0f;
  Tensor m = cpu.to(Device::mps);
  Tensor d = m.clone().detach();
  EXPECT_FALSE(d.is_alias_of(m));
  d.fill(7.0f);
  Tensor back = m.to(Device::cpu);
  auto *bptr = static_cast<float *>(back.data_ptr());
  EXPECT_FLOAT_EQ(bptr[0], 1.0f);
}
#endif

TEST(TensorTest, DivByZeroThrows) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i);
  }
  EXPECT_THROW(static_cast<void>(a.div(b)), std::runtime_error);
  EXPECT_THROW(static_cast<void>(a.div(0.0f)), std::runtime_error);
}

TEST(TensorTest, DivSafeMasksZero) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i);
  }
  Tensor cpu = a.div(b, true);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  EXPECT_FLOAT_EQ(cp[0], 0.0f);
  EXPECT_FLOAT_EQ(cp[1], 2.0f);
  EXPECT_FLOAT_EQ(cp[2], 1.5f);
#ifdef __APPLE__
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor mc = ma.div(mb, true).to(Device::cpu);
  auto *mp = static_cast<float *>(mc.data_ptr());
  EXPECT_FLOAT_EQ(mp[0], 0.0f);
  EXPECT_FLOAT_EQ(mp[1], 2.0f);
  EXPECT_FLOAT_EQ(mp[2], 1.5f);
#endif
}

TEST(TensorTest, DivSafeMasksZeroTruncatedShape) {
  std::array<std::int64_t, 8> shape{3, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i);
  }
  Tensor out = a.div(b, true);
  auto *op = static_cast<float *>(out.data_ptr());
  // Regression: previously yielded {0,1,1.5} after encountering a zero
  // denominator.
  EXPECT_FLOAT_EQ(op[0], 0.0f);
  EXPECT_FLOAT_EQ(op[1], 2.0f);
  EXPECT_FLOAT_EQ(op[2], 1.5f);
}

TEST(TensorTest, DivSafeResetsOffsets) {
  std::array<std::int64_t, 8> shape{3};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i);
  }
  Tensor out = a.div(b, true);
  auto *op = static_cast<float *>(out.data_ptr());
  EXPECT_FLOAT_EQ(op[0], 0.0f);
  EXPECT_FLOAT_EQ(op[1], 2.0f);
  EXPECT_FLOAT_EQ(op[2], 1.5f);
}

TEST(TensorTest, DivSafeVectorSample) {
  std::array<std::int64_t, 8> shape{3};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  float av[3] = {1.f, 2.f, 3.f};
  float bv[3] = {0.f, 1.f, 2.f};
  for (int i = 0; i < 3; ++i) {
    ap[i] = av[i];
    bp[i] = bv[i];
  }
  Tensor out = a.div(b, true);
  auto *op = static_cast<float *>(out.data_ptr());
  EXPECT_FLOAT_EQ(op[0], 0.0f);
  EXPECT_FLOAT_EQ(op[1], 2.0f);
  EXPECT_FLOAT_EQ(op[2], 1.5f);
}

TEST(TensorTest, DivScalarSafeMasksZero) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor cpu = a.div(0.0f, true);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(cp[i], 0.0f);
#ifdef __APPLE__
  Tensor ma = a.to(Device::mps);
  Tensor mc = ma.div(0.0f, true).to(Device::cpu);
  auto *mp = static_cast<float *>(mc.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(mp[i], 0.0f);
#endif
}

#ifdef __APPLE__
TEST(TensorTest, MatmulMetalMatchesCpu) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> bShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(bShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 6; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i + 1);
  }
  Tensor cpu = a.matmul(b);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor mc = ma.matmul(mb).to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mc.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(cp[i], mp[i]);
}

TEST(TensorTest, TransposeMetalMatchesCpu) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 6; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor cpu = a.transpose(0, 1).contiguous();
  Tensor ma = a.to(Device::mps);
  Tensor mc = ma.transpose(0, 1).contiguous().to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mc.data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(cp[i], mp[i]);
}

TEST(TensorTest, SumMetalMatchesCpu) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 4; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor cpu = a.sum();
  Tensor ma = a.to(Device::mps);
  Tensor mb = ma.sum().to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mb.data_ptr());
  EXPECT_FLOAT_EQ(cp[0], mp[0]);
}

TEST(TensorTest, MeanMetalMatchesCpu) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 4; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor cpu = a.mean();
  Tensor ma = a.to(Device::mps);
  Tensor mb = ma.mean().to(Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  auto *mp = static_cast<float *>(mb.data_ptr());
  EXPECT_FLOAT_EQ(cp[0], mp[0]);
}
#endif

TEST(TensorTest, SumAxisDim1) {
  std::array<std::int64_t, 8> shape{2, 3, 4, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 24; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor s = a.sum(1);
  Tensor sk = a.sum(1, true);
  EXPECT_EQ(s.shape()[0], 2);
  EXPECT_EQ(s.shape()[1], 4);
  EXPECT_EQ(sk.shape()[0], 2);
  EXPECT_EQ(sk.shape()[1], 1);
  EXPECT_EQ(sk.shape()[2], 4);
  auto *sp = static_cast<float *>(s.data_ptr());
  auto *skp = static_cast<float *>(sk.data_ptr());
  float exp[8] = {15, 18, 21, 24, 51, 54, 57, 60};
  for (int i = 0; i < 8; ++i) {
    EXPECT_FLOAT_EQ(sp[i], exp[i]);
    EXPECT_FLOAT_EQ(skp[i], exp[i]);
  }
}

TEST(TensorTest, MeanAxisDim1) {
  std::array<std::int64_t, 8> shape{2, 3, 4, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 24; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor m = a.mean(1);
  Tensor mk = a.mean(1, true);
  EXPECT_EQ(m.shape()[0], 2);
  EXPECT_EQ(m.shape()[1], 4);
  EXPECT_EQ(mk.shape()[0], 2);
  EXPECT_EQ(mk.shape()[1], 1);
  EXPECT_EQ(mk.shape()[2], 4);
  auto *mp = static_cast<float *>(m.data_ptr());
  auto *mkp = static_cast<float *>(mk.data_ptr());
  float exp[8] = {5, 6, 7, 8, 17, 18, 19, 20};
  for (int i = 0; i < 8; ++i) {
    EXPECT_FLOAT_EQ(mp[i], exp[i]);
    EXPECT_FLOAT_EQ(mkp[i], exp[i]);
  }
}

TEST(TensorTest, SumMeanAxisDim1) {
  std::array<std::int64_t, 8> shape{2, 3, 4, 0, 0, 0, 0, 0};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 24; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor s = a.sum(1);
  Tensor m = a.mean(1);
  EXPECT_EQ(s.shape()[0], 2);
  EXPECT_EQ(s.shape()[1], 4);
  EXPECT_EQ(m.shape()[0], 2);
  EXPECT_EQ(m.shape()[1], 4);
  Tensor sk = a.sum(1, true);
  Tensor mk = a.mean(1, true);
  EXPECT_EQ(sk.shape()[0], 2);
  EXPECT_EQ(sk.shape()[1], 1);
  EXPECT_EQ(sk.shape()[2], 4);
  EXPECT_EQ(mk.shape()[1], 1);
  EXPECT_EQ(mk.shape()[2], 4);
  auto *sp = static_cast<float *>(s.data_ptr());
  auto *mp = static_cast<float *>(m.data_ptr());
  auto *skp = static_cast<float *>(sk.data_ptr());
  auto *mkp = static_cast<float *>(mk.data_ptr());
  for (int i = 0; i < 2; ++i) {
    for (int k = 0; k < 4; ++k) {
      float rowSum = 0.0f;
      for (int j = 0; j < 3; ++j)
        rowSum += ap[i * 12 + j * 4 + k];
      int idx = i * 4 + k;
      EXPECT_FLOAT_EQ(sp[idx], rowSum);
      EXPECT_FLOAT_EQ(mp[idx], rowSum / 3.0f);
      EXPECT_FLOAT_EQ(skp[idx], rowSum);
      EXPECT_FLOAT_EQ(mkp[idx], rowSum / 3.0f);
    }
  }
#ifdef __APPLE__
  Tensor ma = a.to(Device::mps);
  Tensor ms = ma.sum(1).to(Device::cpu);
  Tensor mm = ma.mean(1).to(Device::cpu);
  Tensor msk = ma.sum(1, true).to(Device::cpu);
  Tensor mmk = ma.mean(1, true).to(Device::cpu);
  EXPECT_EQ(ms.shape()[0], 2);
  EXPECT_EQ(ms.shape()[1], 4);
  EXPECT_EQ(mm.shape()[0], 2);
  EXPECT_EQ(mm.shape()[1], 4);
  EXPECT_EQ(msk.shape()[1], 1);
  EXPECT_EQ(msk.shape()[2], 4);
  EXPECT_EQ(mmk.shape()[1], 1);
  EXPECT_EQ(mmk.shape()[2], 4);
  auto *msp = static_cast<float *>(ms.data_ptr());
  auto *mmp = static_cast<float *>(mm.data_ptr());
  auto *mskp = static_cast<float *>(msk.data_ptr());
  auto *mmkp = static_cast<float *>(mmk.data_ptr());
  for (int i = 0; i < 8; ++i) {
    EXPECT_FLOAT_EQ(sp[i], msp[i]);
    EXPECT_FLOAT_EQ(mp[i], mmp[i]);
    EXPECT_FLOAT_EQ(skp[i], mskp[i]);
    EXPECT_FLOAT_EQ(mkp[i], mmkp[i]);
  }
#endif
}

TEST(TensorTest, SumMeanAxisCpuMetal) {
  std::array<std::int64_t, 8> shape{2, 3, 4, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 24; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor s = a.sum(1);
  Tensor m = a.mean(1);
  EXPECT_EQ(s.shape()[0], 2);
  EXPECT_EQ(s.shape()[1], 4);
  EXPECT_EQ(m.shape()[0], 2);
  EXPECT_EQ(m.shape()[1], 4);
  Tensor sk = a.sum(1, true);
  Tensor mk = a.mean(1, true);
  EXPECT_EQ(sk.shape()[0], 2);
  EXPECT_EQ(sk.shape()[1], 1);
  EXPECT_EQ(sk.shape()[2], 4);
  EXPECT_EQ(mk.shape()[1], 1);
  EXPECT_EQ(mk.shape()[2], 4);
  auto *sp = static_cast<float *>(s.data_ptr());
  auto *mp = static_cast<float *>(m.data_ptr());
  auto *skp = static_cast<float *>(sk.data_ptr());
  auto *mkp = static_cast<float *>(mk.data_ptr());
  for (int i = 0; i < 2; ++i) {
    for (int k = 0; k < 4; ++k) {
      float rowSum = 0.0f;
      for (int j = 0; j < 3; ++j)
        rowSum += ap[i * 12 + j * 4 + k];
      int idx = i * 4 + k;
      EXPECT_FLOAT_EQ(sp[idx], rowSum);
      EXPECT_FLOAT_EQ(mp[idx], rowSum / 3.0f);
      EXPECT_FLOAT_EQ(skp[idx], rowSum);
      EXPECT_FLOAT_EQ(mkp[idx], rowSum / 3.0f);
    }
  }
#ifdef __APPLE__
  Tensor ma = a.to(Device::mps);
  Tensor ms = ma.sum(1).to(Device::cpu);
  Tensor mm = ma.mean(1).to(Device::cpu);
  Tensor msk = ma.sum(1, true).to(Device::cpu);
  Tensor mmk = ma.mean(1, true).to(Device::cpu);
  EXPECT_EQ(ms.shape()[0], 2);
  EXPECT_EQ(ms.shape()[1], 4);
  EXPECT_EQ(mm.shape()[0], 2);
  EXPECT_EQ(mm.shape()[1], 4);
  EXPECT_EQ(msk.shape()[1], 1);
  EXPECT_EQ(msk.shape()[2], 4);
  EXPECT_EQ(mmk.shape()[1], 1);
  EXPECT_EQ(mmk.shape()[2], 4);
  auto *msp = static_cast<float *>(ms.data_ptr());
  auto *mmp = static_cast<float *>(mm.data_ptr());
  auto *mskp = static_cast<float *>(msk.data_ptr());
  auto *mmkp = static_cast<float *>(mmk.data_ptr());
  for (int i = 0; i < 8; ++i) {
    EXPECT_FLOAT_EQ(sp[i], msp[i]);
    EXPECT_FLOAT_EQ(mp[i], mmp[i]);
    EXPECT_FLOAT_EQ(skp[i], mskp[i]);
    EXPECT_FLOAT_EQ(mkp[i], mmkp[i]);
  }
#endif
}

TEST(TensorTest, SumMeanAxisBareShape) {
  std::array<std::int64_t, 8> shape{2, 3, 4};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 24; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor s = a.sum(1);
  Tensor m = a.mean(1);
  EXPECT_EQ(s.shape()[0], 2);
  EXPECT_EQ(s.shape()[1], 4);
  EXPECT_EQ(m.shape()[0], 2);
  EXPECT_EQ(m.shape()[1], 4);
  Tensor sk = a.sum(1, true);
  Tensor mk = a.mean(1, true);
  EXPECT_EQ(sk.shape()[0], 2);
  EXPECT_EQ(sk.shape()[1], 1);
  EXPECT_EQ(sk.shape()[2], 4);
  EXPECT_EQ(mk.shape()[1], 1);
  EXPECT_EQ(mk.shape()[2], 4);
  auto *sp = static_cast<float *>(s.data_ptr());
  auto *mp = static_cast<float *>(m.data_ptr());
  auto *skp = static_cast<float *>(sk.data_ptr());
  auto *mkp = static_cast<float *>(mk.data_ptr());
  for (int i = 0; i < 2; ++i) {
    for (int k = 0; k < 4; ++k) {
      float rowSum = 0.0f;
      for (int j = 0; j < 3; ++j)
        rowSum += ap[i * 12 + j * 4 + k];
      int idx = i * 4 + k;
      EXPECT_FLOAT_EQ(sp[idx], rowSum);
      EXPECT_FLOAT_EQ(mp[idx], rowSum / 3.0f);
      EXPECT_FLOAT_EQ(skp[idx], rowSum);
      EXPECT_FLOAT_EQ(mkp[idx], rowSum / 3.0f);
    }
  }
}

TEST(TensorTest, FillCpu) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  t.fill(3.0f);
  auto *p = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(p[i], 3.0f);
}
#ifdef __APPLE__
TEST(TensorTest, FillMetalMatchesCpu) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor m = t.to(Device::mps);
  m.fill(5.0f);
  Tensor c = m.to(Device::cpu);
  auto *p = static_cast<float *>(c.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(p[i], 5.0f);
}

TEST(TensorTest, AutogradAddMetal) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i);
    bp[i] = static_cast<float>(i * 2);
  }
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor c = ma.add(mb);
  c.backward();
  Tensor ag = ma.grad().to(Device::cpu);
  Tensor bg = mb.grad().to(Device::cpu);
  auto *agp = static_cast<float *>(ag.data_ptr());
  auto *bgp = static_cast<float *>(bg.data_ptr());
  for (int i = 0; i < 3; ++i) {
    EXPECT_FLOAT_EQ(agp[i], 1.0f);
    EXPECT_FLOAT_EQ(bgp[i], 1.0f);
  }
}

TEST(TensorTest, AutogradMulMetal) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor ac = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor bc = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(ac.data_ptr());
  auto *bp = static_cast<float *>(bc.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i + 2);
  }
  ac.set_requires_grad(true);
  bc.set_requires_grad(true);
  Tensor cc = ac.mul(bc);
  cc.backward();
  Tensor ag_exp = ac.grad();
  Tensor bg_exp = bc.grad();

  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  std::memcpy(a.data_ptr(), ac.data_ptr(), 3 * sizeof(float));
  std::memcpy(b.data_ptr(), bc.data_ptr(), 3 * sizeof(float));
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor c = ma.mul(mb);
  c.backward();
  Tensor ag = ma.grad().to(Device::cpu);
  Tensor bg = mb.grad().to(Device::cpu);
  auto *agp = static_cast<float *>(ag.data_ptr());
  auto *bgp = static_cast<float *>(bg.data_ptr());
  auto *ag_exp_p = static_cast<float *>(ag_exp.data_ptr());
  auto *bg_exp_p = static_cast<float *>(bg_exp.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(agp[i], ag_exp_p[i]);
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(bgp[i], bg_exp_p[i]);
}

TEST(TensorTest, AutogradDivMetal) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor ac = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor bc = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(ac.data_ptr());
  auto *bp = static_cast<float *>(bc.data_ptr());
  for (int i = 0; i < 3; ++i) {
    ap[i] = static_cast<float>(i + 2);
    bp[i] = static_cast<float>(i + 1);
  }
  ac.set_requires_grad(true);
  bc.set_requires_grad(true);
  Tensor cc = ac.div(bc);
  cc.backward();
  Tensor ag_exp = ac.grad();
  Tensor bg_exp = bc.grad();

  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  std::memcpy(a.data_ptr(), ac.data_ptr(), 3 * sizeof(float));
  std::memcpy(b.data_ptr(), bc.data_ptr(), 3 * sizeof(float));
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor c = ma.div(mb);
  c.backward();
  Tensor ag = ma.grad().to(Device::cpu);
  Tensor bg = mb.grad().to(Device::cpu);
  auto *agp = static_cast<float *>(ag.data_ptr());
  auto *bgp = static_cast<float *>(bg.data_ptr());
  auto *ag_exp_p = static_cast<float *>(ag_exp.data_ptr());
  auto *bg_exp_p = static_cast<float *>(bg_exp.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(agp[i], ag_exp_p[i]);
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(bgp[i], bg_exp_p[i]);
}

TEST(TensorTest, AutogradDivScalarMetal) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor ac = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(ac.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 2);
  ac.set_requires_grad(true);
  Tensor cc = ac.div(2.0f);
  cc.backward();
  Tensor ag_exp = ac.grad();

  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  std::memcpy(a.data_ptr(), ac.data_ptr(), 3 * sizeof(float));
  a.set_requires_grad(true);
  Tensor ma = a.to(Device::mps);
  Tensor c = ma.div(2.0f);
  c.backward();
  Tensor ag = ma.grad().to(Device::cpu);
  auto *agp = static_cast<float *>(ag.data_ptr());
  auto *ag_exp_p = static_cast<float *>(ag_exp.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(agp[i], ag_exp_p[i]);
}

TEST(TensorTest, AutogradDetachMetal) {
  std::array<std::int64_t, 8> shape{3, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 3; ++i)
    ap[i] = static_cast<float>(i + 1);
  Tensor ma = a.to(Device::mps);
  ma.set_requires_grad(true);
  Tensor b = ma.detach();
  b.set_requires_grad(true);
  Tensor c = b.mul(b);
  c.backward();
  EXPECT_EQ(ma.grad().data_ptr(), nullptr);
  Tensor bg = b.grad().to(Device::cpu);
  auto *bgp = static_cast<float *>(bg.data_ptr());
  for (int i = 0; i < 3; ++i)
    EXPECT_FLOAT_EQ(bgp[i], 2 * ap[i]);
}

TEST(TensorTest, AutogradMatmulMetal) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> bShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor ac = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor bc = Tensor::empty(bShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(ac.data_ptr());
  auto *bp = static_cast<float *>(bc.data_ptr());
  for (int i = 0; i < 6; ++i) {
    ap[i] = static_cast<float>(i + 1);
    bp[i] = static_cast<float>(i + 1);
  }
  ac.set_requires_grad(true);
  bc.set_requires_grad(true);
  Tensor cc = ac.matmul(bc);
  cc.backward();
  Tensor ag_exp = ac.grad();
  Tensor bg_exp = bc.grad();

  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(bShape, DType::f32, Device::cpu);
  std::memcpy(a.data_ptr(), ac.data_ptr(), 6 * sizeof(float));
  std::memcpy(b.data_ptr(), bc.data_ptr(), 6 * sizeof(float));
  a.set_requires_grad(true);
  b.set_requires_grad(true);
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor c = ma.matmul(mb);
  c.backward();
  Tensor ag = ma.grad().to(Device::cpu);
  Tensor bg = mb.grad().to(Device::cpu);
  auto *agp = static_cast<float *>(ag.data_ptr());
  auto *bgp = static_cast<float *>(bg.data_ptr());
  auto *ag_exp_p = static_cast<float *>(ag_exp.data_ptr());
  auto *bg_exp_p = static_cast<float *>(bg_exp.data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(agp[i], ag_exp_p[i]);
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(bgp[i], bg_exp_p[i]);
}

TEST(TensorTest, AutogradSumMetal) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor tc = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *cp = static_cast<float *>(tc.data_ptr());
  for (int i = 0; i < 4; ++i)
    cp[i] = static_cast<float>(i + 1);
  tc.set_requires_grad(true);
  Tensor sc = tc.sum();
  sc.backward();
  Tensor g_exp = tc.grad();

  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  std::memcpy(t.data_ptr(), tc.data_ptr(), 4 * sizeof(float));
  t.set_requires_grad(true);
  Tensor m = t.to(Device::mps);
  Tensor s = m.sum();
  s.backward();
  Tensor g = m.grad().to(Device::cpu);
  auto *gp = static_cast<float *>(g.data_ptr());
  auto *exp_p = static_cast<float *>(g_exp.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(gp[i], exp_p[i]);
}

TEST(TensorTest, AutogradMeanMetal) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor tc = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *cp = static_cast<float *>(tc.data_ptr());
  for (int i = 0; i < 4; ++i)
    cp[i] = static_cast<float>(i + 1);
  tc.set_requires_grad(true);
  Tensor sc = tc.mean();
  sc.backward();
  Tensor g_exp = tc.grad();

  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  std::memcpy(t.data_ptr(), tc.data_ptr(), 4 * sizeof(float));
  t.set_requires_grad(true);
  Tensor m = t.to(Device::mps);
  Tensor s = m.mean();
  Tensor mc = s.to(Device::cpu);
  auto *mp = static_cast<float *>(mc.data_ptr());
  auto *cp2 = static_cast<float *>(sc.data_ptr());
  EXPECT_FLOAT_EQ(mp[0], cp2[0]);
  s.backward();
  Tensor g = m.grad().to(Device::cpu);
  auto *gp = static_cast<float *>(g.data_ptr());
  auto *exp_p = static_cast<float *>(g_exp.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(gp[i], exp_p[i]);
}

TEST(TensorTest, AutogradViewMetal) {
  std::array<std::int64_t, 8> shape{2, 2, 1, 1, 1, 1, 1, 1};
  Tensor tc = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *cp = static_cast<float *>(tc.data_ptr());
  for (int i = 0; i < 4; ++i)
    cp[i] = 1.0f;
  tc.set_requires_grad(true);
  std::array<std::int64_t, 8> newShape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor vc = tc.view(newShape);
  Tensor sc = vc.sum();
  sc.backward();
  Tensor g_exp = tc.grad();

  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  std::memcpy(t.data_ptr(), tc.data_ptr(), 4 * sizeof(float));
  t.set_requires_grad(true);
  Tensor m = t.to(Device::mps);
  Tensor v = m.view(newShape);
  Tensor s = v.sum();
  s.backward();
  Tensor g = m.grad().to(Device::cpu);
  auto *gp = static_cast<float *>(g.data_ptr());
  auto *exp_p = static_cast<float *>(g_exp.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(gp[i], exp_p[i]);
}

TEST(TensorTest, AutogradTransposeMetal) {
  std::array<std::int64_t, 8> aShape{2, 3, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> cShape{3, 2, 1, 1, 1, 1, 1, 1};
  Tensor ac = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor cc = Tensor::empty(cShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(ac.data_ptr());
  auto *cp = static_cast<float *>(cc.data_ptr());
  for (int i = 0; i < 6; ++i) {
    ap[i] = static_cast<float>(i + 1);
    cp[i] = static_cast<float>(i + 1);
  }
  ac.set_requires_grad(true);
  Tensor tc = ac.transpose(0, 1);
  Tensor pc = tc.mul(cc);
  Tensor sc = pc.sum();
  sc.backward();
  Tensor ag_exp = ac.grad();

  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor c = Tensor::empty(cShape, DType::f32, Device::cpu);
  std::memcpy(a.data_ptr(), ac.data_ptr(), 6 * sizeof(float));
  std::memcpy(c.data_ptr(), cc.data_ptr(), 6 * sizeof(float));
  a.set_requires_grad(true);
  Tensor ma = a.to(Device::mps);
  Tensor mc = c.to(Device::mps);
  Tensor t = ma.transpose(0, 1);
  Tensor p = t.mul(mc);
  Tensor s = p.sum();
  s.backward();
  Tensor ag = ma.grad().to(Device::cpu);
  auto *agp = static_cast<float *>(ag.data_ptr());
  auto *ag_exp_p = static_cast<float *>(ag_exp.data_ptr());
  for (int i = 0; i < 6; ++i)
    EXPECT_FLOAT_EQ(agp[i], ag_exp_p[i]);
}
#endif

TEST(AllocatorTest, ArenaStress) {
  orchard::runtime::CpuAllocator alloc;
  for (int i = 0; i < 100000; ++i) {
    void *p = alloc.allocate(64, "stress");
    alloc.deallocate(p, "stress");
  }
  SUCCEED();
}

TEST(TensorTest, ContiguousPreservesData) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *base = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 4; ++i)
    base[i] = static_cast<float>(i);
  Tensor s = t.slice(0, 0, 4, 2);
  EXPECT_FALSE(s.is_contiguous());
  Tensor c = s.contiguous();
  auto *ptr = static_cast<float *>(c.data_ptr());
  EXPECT_FLOAT_EQ(ptr[0], 0.0f);
  EXPECT_FLOAT_EQ(ptr[1], 2.0f);
}

TEST(RuntimeTest, CpuContextAdd) {
  float a[3] = {1.0f, 2.0f, 3.0f};
  float b[3] = {4.0f, 5.0f, 6.0f};
  float c[3] = {0.0f, 0.0f, 0.0f};
  orchard::runtime::cpu_context().add(a, b, c, 3);
  EXPECT_FLOAT_EQ(c[0], 5.0f);
  EXPECT_FLOAT_EQ(c[1], 7.0f);
  EXPECT_FLOAT_EQ(c[2], 9.0f);
}

TEST(RuntimeTest, MetalContextQueuePooling) {
  auto &ctx = orchard::runtime::metal_context();
  auto q1 = ctx.acquire_command_queue();
  ctx.return_command_queue(q1);
  auto q2 = ctx.acquire_command_queue();
  EXPECT_EQ(q1, q2);
  ctx.return_command_queue(q2);
}

TEST(TensorBroadcastTest, ScalarTensor) {
  std::array<std::int64_t, 8> sShape{1, 1, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> tShape{2, 3, 1, 1, 1, 1, 1, 1};
  Tensor s = Tensor::empty(sShape, DType::f32, Device::cpu);
  Tensor t = Tensor::empty(tShape, DType::f32, Device::cpu);
  *static_cast<float *>(s.data_ptr()) = 2.0f;
  auto *tp = static_cast<float *>(t.data_ptr());
  for (int i = 0; i < 6; ++i)
    tp[i] = static_cast<float>(i);
  Tensor add = s.add(t);
  Tensor mul = s.mul(t);
  Tensor div = t.div(s);
  auto *ap = static_cast<float *>(add.data_ptr());
  auto *mp = static_cast<float *>(mul.data_ptr());
  auto *dp = static_cast<float *>(div.data_ptr());
  for (int i = 0; i < 6; ++i) {
    EXPECT_FLOAT_EQ(ap[i], tp[i] + 2.0f);
    EXPECT_FLOAT_EQ(mp[i], tp[i] * 2.0f);
    EXPECT_FLOAT_EQ(dp[i], tp[i] / 2.0f);
  }
  Tensor ms = s.to(Device::mps);
  Tensor mt = t.to(Device::mps);
  Tensor madd = ms.add(mt).to(Device::cpu);
  Tensor mmul = ms.mul(mt).to(Device::cpu);
  Tensor mdiv = mt.div(ms).to(Device::cpu);
  auto *map = static_cast<float *>(madd.data_ptr());
  auto *mmp = static_cast<float *>(mmul.data_ptr());
  auto *mdp = static_cast<float *>(mdiv.data_ptr());
  for (int i = 0; i < 6; ++i) {
    EXPECT_FLOAT_EQ(map[i], tp[i] + 2.0f);
    EXPECT_FLOAT_EQ(mmp[i], tp[i] * 2.0f);
    EXPECT_FLOAT_EQ(mdp[i], tp[i] / 2.0f);
  }
}

TEST(TensorBroadcastTest, VectorMatrix) {
  std::array<std::int64_t, 8> vShape{1, 4, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> mShape{3, 4, 1, 1, 1, 1, 1, 1};
  Tensor v = Tensor::empty(vShape, DType::f32, Device::cpu);
  Tensor m = Tensor::empty(mShape, DType::f32, Device::cpu);
  auto *vp = static_cast<float *>(v.data_ptr());
  auto *mp = static_cast<float *>(m.data_ptr());
  for (int i = 0; i < 4; ++i)
    vp[i] = static_cast<float>(i + 1);
  for (int i = 0; i < 12; ++i)
    mp[i] = static_cast<float>(i);
  Tensor add = m.add(v);
  Tensor mul = v.mul(m);
  Tensor div = m.div(v);
  auto *addp = static_cast<float *>(add.data_ptr());
  auto *mulp = static_cast<float *>(mul.data_ptr());
  auto *divp = static_cast<float *>(div.data_ptr());
  for (int r = 0; r < 3; ++r) {
    for (int c = 0; c < 4; ++c) {
      int idx = r * 4 + c;
      float vv = vp[c];
      float mv = mp[idx];
      EXPECT_FLOAT_EQ(addp[idx], mv + vv);
      EXPECT_FLOAT_EQ(mulp[idx], mv * vv);
      EXPECT_FLOAT_EQ(divp[idx], mv / vv);
    }
  }
  Tensor mv = v.to(Device::mps);
  Tensor mm = m.to(Device::mps);
  Tensor madd = mm.add(mv).to(Device::cpu);
  Tensor mmul = mv.mul(mm).to(Device::cpu);
  Tensor mdiv = mm.div(mv).to(Device::cpu);
  auto *maddp = static_cast<float *>(madd.data_ptr());
  auto *mmulp = static_cast<float *>(mmul.data_ptr());
  auto *mdivp = static_cast<float *>(mdiv.data_ptr());
  for (int r = 0; r < 3; ++r) {
    for (int c = 0; c < 4; ++c) {
      int idx = r * 4 + c;
      float vv = vp[c];
      float mvv = mp[idx];
      EXPECT_FLOAT_EQ(maddp[idx], mvv + vv);
      EXPECT_FLOAT_EQ(mmulp[idx], mvv * vv);
      EXPECT_FLOAT_EQ(mdivp[idx], mvv / vv);
    }
  }
}

TEST(TensorBroadcastTest, HigherRank) {
  std::array<std::int64_t, 8> aShape{2, 1, 3, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> bShape{1, 4, 1, 5, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(bShape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 6; ++i)
    ap[i] = static_cast<float>(i + 1);
  for (int i = 0; i < 20; ++i)
    bp[i] = static_cast<float>(i + 2);
  Tensor add = a.add(b);
  Tensor mul = a.mul(b);
  Tensor div = a.div(b);
  auto *addp = static_cast<float *>(add.data_ptr());
  auto *mulp = static_cast<float *>(mul.data_ptr());
  auto *divp = static_cast<float *>(div.data_ptr());
  for (int i0 = 0; i0 < 2; ++i0) {
    for (int i1 = 0; i1 < 4; ++i1) {
      for (int i2 = 0; i2 < 3; ++i2) {
        for (int i3 = 0; i3 < 5; ++i3) {
          int outIdx = ((i0 * 4 + i1) * 3 + i2) * 5 + i3;
          float av = ap[i0 * 3 + i2];
          float bv = bp[i1 * 5 + i3];
          EXPECT_FLOAT_EQ(addp[outIdx], av + bv);
          EXPECT_FLOAT_EQ(mulp[outIdx], av * bv);
          EXPECT_FLOAT_EQ(divp[outIdx], av / bv);
        }
      }
    }
  }
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor madd = ma.add(mb).to(Device::cpu);
  Tensor mmul = ma.mul(mb).to(Device::cpu);
  Tensor mdiv = ma.div(mb).to(Device::cpu);
  auto *maddp = static_cast<float *>(madd.data_ptr());
  auto *mmulp = static_cast<float *>(mmul.data_ptr());
  auto *mdivp = static_cast<float *>(mdiv.data_ptr());
  for (int i0 = 0; i0 < 2; ++i0) {
    for (int i1 = 0; i1 < 4; ++i1) {
      for (int i2 = 0; i2 < 3; ++i2) {
        for (int i3 = 0; i3 < 5; ++i3) {
          int outIdx = ((i0 * 4 + i1) * 3 + i2) * 5 + i3;
          float av = ap[i0 * 3 + i2];
          float bv = bp[i1 * 5 + i3];
          EXPECT_FLOAT_EQ(maddp[outIdx], av + bv);
          EXPECT_FLOAT_EQ(mmulp[outIdx], av * bv);
          EXPECT_FLOAT_EQ(mdivp[outIdx], av / bv);
        }
      }
    }
  }
}

TEST(TensorTest, ProfilingLogCreation) {
  orchard::tensor_profile_clear_log();
  unsetenv("ORCHARD_TENSOR_PROFILE");
  orchard::tensor_profile_reset();
  std::array<std::int64_t, 8> shape{1, 1, 1, 1, 1, 1, 1, 1};
  {
    Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
    (void)t;
  }
  dump_live_tensors();
  std::ifstream off("/tmp/orchard_tensor_profile.log");
  EXPECT_FALSE(off.good());
  setenv("ORCHARD_TENSOR_PROFILE", "1", 1);
  orchard::tensor_profile_reset();
  {
    Tensor t = Tensor::empty(shape, DType::f32, Device::cpu);
    (void)t;
  }
  dump_live_tensors();
  std::ifstream ifs("/tmp/orchard_tensor_profile.log");
  EXPECT_TRUE(ifs.good());
  unsetenv("ORCHARD_TENSOR_PROFILE");
  orchard::tensor_profile_reset();
}

TEST(TensorTest, ProfilingLogEntries) {
  orchard::tensor_profile_clear_log();
  unsetenv("ORCHARD_TENSOR_PROFILE");
  orchard::tensor_profile_reset();
  std::array<std::int64_t, 8> shape{1, 1, 1, 1, 1, 1, 1, 1};
  {
    Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
    Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
    dump_live_tensors();
    (void)a;
    (void)b;
  }
  dump_live_tensors();
  std::ifstream pre("/tmp/orchard_tensor_profile.log");
  EXPECT_FALSE(pre.good());
  setenv("ORCHARD_TENSOR_PROFILE", "1", 1);
  orchard::tensor_profile_reset();
  {
    Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
    Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
    dump_live_tensors();
    (void)a;
    (void)b;
  }
  dump_live_tensors();
  unsetenv("ORCHARD_TENSOR_PROFILE");
  orchard::tensor_profile_reset();

  std::ifstream ifs("/tmp/orchard_tensor_profile.log");
  ASSERT_TRUE(ifs.good());
  std::stringstream buffer;
  buffer << ifs.rdbuf();
  std::string contents = buffer.str();
  EXPECT_NE(contents.find("alloc"), std::string::npos);
  EXPECT_NE(contents.find("free"), std::string::npos);
  EXPECT_NE(contents.find("live"), std::string::npos);
}
