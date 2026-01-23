#pragma once

#include <memory>
#include <vector>

namespace orchard {
namespace core {
namespace tensor {
class Tensor;
}
namespace autograd {

struct Node;

struct Edge {
  std::shared_ptr<Node> fn;
  explicit Edge(std::shared_ptr<Node> f);
};

struct Node : std::enable_shared_from_this<Node> {
  virtual ~Node() = default;
  virtual void apply(tensor::Tensor &grad) = 0;

protected:
  static void accumulate(tensor::Tensor &t, const tensor::Tensor &grad);
};

/// Execute backward pass starting from the given root tensor.
void backward(tensor::Tensor &root);

} // namespace autograd
} // namespace core
} // namespace orchard
