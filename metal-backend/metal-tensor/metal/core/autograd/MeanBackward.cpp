#include "MeanBackward.h"
#include "../../runtime/MetalKernels.h"

#include <cstddef>

using namespace orchard::core::tensor;

namespace orchard::core::autograd {

MeanBackward::MeanBackward(const Tensor &aa)
    : a(aa), pa(const_cast<Tensor *>(&aa)) {}
MeanBackward::MeanBackward(const Tensor &aa, int d, bool k)
    : a(aa), pa(const_cast<Tensor *>(&aa)), dim(d), keepdim(k),
      reduce_all(false) {}

void MeanBackward::apply(Tensor &g) {
  if (reduce_all) {
    Tensor grad = Tensor::empty(a.shape(), DType::f32, g.device());
    Tensor g_cpu = g.to(Device::cpu);
    float v = *static_cast<float *>(g_cpu.data_ptr());
    v /= static_cast<float>(a.numel());
    if (g.device() == Device::mps) {
      try {
        runtime::metal_fill(static_cast<float *>(grad.data_ptr()), v,
                            a.numel());
      } catch (const std::runtime_error &) {
        auto *ptr = static_cast<float *>(grad.data_ptr());
        for (std::size_t i = 0; i < a.numel(); ++i)
          ptr[i] = v;
      }
    } else {
      auto *ptr = static_cast<float *>(grad.data_ptr());
      for (std::size_t i = 0; i < a.numel(); ++i)
        ptr[i] = v;
    }
    accumulate(*pa, grad.to(pa->device()));
  } else {
    Tensor gv = g;
    if (!keepdim) {
      auto shp = g.shape();
      for (int i = 7; i > dim; --i)
        shp[i] = shp[i - 1];
      shp[dim] = 1;
      gv = g.view(shp);
    }
    Tensor base = Tensor::empty(a.shape(), DType::f32, g.device());
    base.fill(0.0f);
    Tensor grad = base.add(gv);
    float scale = 1.0f / static_cast<float>(a.shape()[dim]);
    Tensor sc = Tensor::empty({1, 1, 1, 1, 1, 1, 1, 1}, DType::f32, g.device());
    *static_cast<float *>(sc.data_ptr()) = scale;
    grad = grad.mul(sc);
    accumulate(*pa, grad.to(pa->device()));
  }
  if (pa->grad_fn() && pa->grad_fn().get() != this)
    pa->grad_fn()->apply(pa->grad());
}

} // namespace orchard::core::autograd
