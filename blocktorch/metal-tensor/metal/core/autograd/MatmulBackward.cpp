#include "MatmulBackward.h"
#include "../../runtime/MetalKernels.h"

#include <cstddef>

using namespace orchard::core::tensor;

namespace orchard::core::autograd {

MatmulBackward::MatmulBackward(const Tensor &aa, const Tensor &bb)
    : a(aa), b(bb), pa(const_cast<Tensor *>(&aa)),
      pb(const_cast<Tensor *>(&bb)) {}

void MatmulBackward::apply(Tensor &g) {
  auto m = a.shape()[0];
  auto n = b.shape()[1];
  auto k = a.shape()[1];
  Device dev = g.device();
  Tensor ga = Tensor::empty(a.shape(), DType::f32, dev);
  Tensor gb = Tensor::empty(b.shape(), DType::f32, dev);
  Tensor aa = a.to(dev);
  Tensor bb = b.to(dev);
  if (dev == Device::mps) {
    try {
      runtime::metal_matmul_backward_a(
          static_cast<const float *>(g.data_ptr()),
          static_cast<const float *>(bb.data_ptr()),
          static_cast<float *>(ga.data_ptr()), m, n, k);
      runtime::metal_matmul_backward_b(
          static_cast<const float *>(g.data_ptr()),
          static_cast<const float *>(aa.data_ptr()),
          static_cast<float *>(gb.data_ptr()), m, n, k);
    } catch (const std::runtime_error &) {
      const auto *gp = static_cast<const float *>(g.data_ptr());
      const auto *bp = static_cast<const float *>(bb.data_ptr());
      const auto *ap = static_cast<const float *>(aa.data_ptr());
      auto *gap = static_cast<float *>(ga.data_ptr());
      auto *gbp = static_cast<float *>(gb.data_ptr());
      for (std::size_t i = 0; i < static_cast<std::size_t>(m); ++i) {
        for (std::size_t j = 0; j < static_cast<std::size_t>(k); ++j) {
          float s = 0.0f;
          for (std::size_t p = 0; p < static_cast<std::size_t>(n); ++p)
            s += gp[i * n + p] * bp[j * n + p];
          gap[i * k + j] = s;
        }
      }
      for (std::size_t i = 0; i < static_cast<std::size_t>(k); ++i) {
        for (std::size_t j = 0; j < static_cast<std::size_t>(n); ++j) {
          float s = 0.0f;
          for (std::size_t p = 0; p < static_cast<std::size_t>(m); ++p)
            s += ap[p * k + i] * gp[p * n + j];
          gbp[i * n + j] = s;
        }
      }
    }
  } else {
    const auto *gp = static_cast<const float *>(g.data_ptr());
    const auto *bp = static_cast<const float *>(bb.data_ptr());
    const auto *ap = static_cast<const float *>(aa.data_ptr());
    auto *gap = static_cast<float *>(ga.data_ptr());
    auto *gbp = static_cast<float *>(gb.data_ptr());
    for (std::size_t i = 0; i < static_cast<std::size_t>(m); ++i) {
      for (std::size_t j = 0; j < static_cast<std::size_t>(k); ++j) {
        float s = 0.0f;
        for (std::size_t p = 0; p < static_cast<std::size_t>(n); ++p)
          s += gp[i * n + p] * bp[j * n + p];
        gap[i * k + j] = s;
      }
    }
    for (std::size_t i = 0; i < static_cast<std::size_t>(k); ++i) {
      for (std::size_t j = 0; j < static_cast<std::size_t>(n); ++j) {
        float s = 0.0f;
        for (std::size_t p = 0; p < static_cast<std::size_t>(m); ++p)
          s += ap[p * k + i] * gp[p * n + j];
        gbp[i * n + j] = s;
      }
    }
  }
  accumulate(*pa, ga.to(pa->device()));
  accumulate(*pb, gb.to(pb->device()));
  if (pa->grad_fn() && pa->grad_fn().get() != this)
    pa->grad_fn()->apply(pa->grad());
  if (pb->grad_fn() && pb->grad_fn().get() != this)
    pb->grad_fn()->apply(pb->grad());
}

} // namespace orchard::core::autograd
