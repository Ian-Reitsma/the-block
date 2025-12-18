#include "ViewBackward.h"

using namespace orchard::core::tensor;

namespace orchard::core::autograd {

ViewBackward::ViewBackward(const Tensor &b)
    : base(b), pbase(const_cast<Tensor *>(&b)) {}

void ViewBackward::apply(Tensor &g) {
  Tensor reshaped = g.view(base.shape());
  accumulate(*pbase, reshaped);
  if (pbase->grad_fn() && pbase->grad_fn().get() != this)
    pbase->grad_fn()->apply(pbase->grad());
}

} // namespace orchard::core::autograd
