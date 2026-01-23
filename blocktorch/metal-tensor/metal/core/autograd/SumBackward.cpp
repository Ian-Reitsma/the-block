#include "SumBackward.h"
#include "../../runtime/MetalKernels.h"

#include <cstddef>

using namespace orchard::core::tensor;

namespace orchard::core::autograd {

SumBackward::SumBackward(const Tensor &aa)
    : a(aa), pa(const_cast<Tensor *>(&aa)) {}
SumBackward::SumBackward(const Tensor &aa, int d, bool k)
    : a(aa), pa(const_cast<Tensor *>(&aa)), dim(d), keepdim(k),
      reduce_all(false) {}

void SumBackward::apply(Tensor &g) {
  if (reduce_all) {
    Tensor grad = Tensor::empty(a.shape(), DType::f32, g.device());
    Tensor g_cpu = g.to(Device::cpu);
    float v = *static_cast<float *>(g_cpu.data_ptr());
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
    accumulate(*pa, grad.to(pa->device()));
  }
  if (pa->grad_fn() && pa->grad_fn().get() != this)
    pa->grad_fn()->apply(pa->grad());
}

} // namespace orchard::core::autograd
