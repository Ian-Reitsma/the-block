#pragma once

#include "../tensor/Tensor.h"
#include "Node.h"

namespace orchard::core::autograd {

struct MeanBackward : Node {
  tensor::Tensor a;
  tensor::Tensor *pa;
  int dim{0};
  bool keepdim{false};
  bool reduce_all{true};
  explicit MeanBackward(const tensor::Tensor &aa);
  MeanBackward(const tensor::Tensor &aa, int d, bool k);
  void apply(tensor::Tensor &g) override;
};

} // namespace orchard::core::autograd
