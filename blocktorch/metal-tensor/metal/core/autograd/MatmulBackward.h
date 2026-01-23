#pragma once

#include "../tensor/Tensor.h"
#include "Node.h"

namespace orchard::core::autograd {

struct MatmulBackward : Node {
  tensor::Tensor a;
  tensor::Tensor b;
  tensor::Tensor *pa;
  tensor::Tensor *pb;
  MatmulBackward(const tensor::Tensor &aa, const tensor::Tensor &bb);
  void apply(tensor::Tensor &g) override;
};

} // namespace orchard::core::autograd
