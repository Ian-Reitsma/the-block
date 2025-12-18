#pragma once

#include "../tensor/Tensor.h"
#include "Node.h"

namespace orchard::core::autograd {

struct DivBackward : Node {
  tensor::Tensor a;
  tensor::Tensor b;
  tensor::Tensor *pa;
  tensor::Tensor *pb;
  bool safe{false};
  DivBackward(const tensor::Tensor &aa, const tensor::Tensor &bb, bool s);
  void apply(tensor::Tensor &g) override;
};

} // namespace orchard::core::autograd
