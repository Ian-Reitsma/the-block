#pragma once

#include "../tensor/Tensor.h"
#include "Node.h"

namespace orchard::core::autograd {

struct TransposeBackward : Node {
  tensor::Tensor base;
  tensor::Tensor *pbase;
  int dim0;
  int dim1;
  TransposeBackward(const tensor::Tensor &b, int d0, int d1);
  void apply(tensor::Tensor &g) override;
};

} // namespace orchard::core::autograd
