#include "TransposeBackward.h"
#include "../../runtime/MetalKernels.h"

using namespace orchard::core::tensor;

namespace orchard::core::autograd {

TransposeBackward::TransposeBackward(const Tensor &b, int d0, int d1)
    : base(b), pbase(const_cast<Tensor *>(&b)), dim0(d0), dim1(d1) {}

void TransposeBackward::apply(Tensor &g) {
  Tensor gg = g.to(pbase->device());
  Tensor out;
  std::size_t m = static_cast<std::size_t>(base.shape()[dim0]);
  std::size_t n = static_cast<std::size_t>(base.shape()[dim1]);
  if (gg.device() == Device::cpu) {
    out = gg.transpose(dim1, dim0).detach();
  } else {
    out = Tensor::empty(base.shape(), base.dtype(), pbase->device());
    try {
      runtime::metal_transpose_backward(
          static_cast<const float *>(gg.data_ptr()),
          static_cast<float *>(out.data_ptr()), m, n);
    } catch (const std::runtime_error &) {
      out = gg.transpose(dim1, dim0).detach();
    }
  }
  if (pbase->grad_fn() && pbase->grad_fn().get() != this)
    pbase->grad_fn()->apply(out);
  else
    accumulate(*pbase, out);
}

} // namespace orchard::core::autograd
