#pragma once

#include "../tensor/Tensor.h"
#include "Node.h"

namespace orchard::core::autograd {

struct SumBackward : Node {
  tensor::Tensor a;
  tensor::Tensor *pa;
  int dim{0};
  bool keepdim{false};
  bool reduce_all{true};
  explicit SumBackward(const tensor::Tensor &aa);
  SumBackward(const tensor::Tensor &aa, int d, bool k);
  void apply(tensor::Tensor &g) override;
};

} // namespace orchard::core::autograd
