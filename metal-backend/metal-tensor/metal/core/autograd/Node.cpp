#include "Node.h"
#include "../../runtime/CpuContext.h"
#include "../../runtime/MetalKernels.h"
#include "../tensor/Tensor.h"

#include <cstdint>
#include <stdexcept>

using namespace orchard::core::tensor;

namespace orchard::core::autograd {

Edge::Edge(std::shared_ptr<Node> f) : fn(std::move(f)) {}

void Node::accumulate(Tensor &t, const Tensor &grad) {
  if (!t.requires_grad())
    return;
  if (!t.grad().data_ptr())
    t.set_grad(Tensor::zerosLike(t));
  auto n = t.numel();
  if (t.grad().device() == Device::mps) {
    auto shape = t.shape();
    auto strides = t.strides();
    std::uint32_t dims = 0;
    for (auto s : shape) {
      if (s <= 0)
        break;
      ++dims;
    }
    try {
      orchard::runtime::metal_add(
          static_cast<const float *>(grad.data_ptr()),
          static_cast<const float *>(t.grad().data_ptr()),
          static_cast<float *>(t.grad().data_ptr()), shape.data(),
          strides.data(), strides.data(), dims, n);
    } catch (const std::runtime_error &) {
      orchard::runtime::cpu_context().add(
          static_cast<const float *>(grad.data_ptr()),
          static_cast<const float *>(t.grad().data_ptr()),
          static_cast<float *>(t.grad().data_ptr()), n);
    }
  } else {
    orchard::runtime::cpu_context().add(
        static_cast<const float *>(grad.data_ptr()),
        static_cast<const float *>(t.grad().data_ptr()),
        static_cast<float *>(t.grad().data_ptr()), n);
  }
}

void backward(Tensor &root) {
  if (!root.requires_grad())
    return;
  Tensor g;
  if (root.grad().data_ptr()) {
    g = root.grad();
  } else {
    Tensor ones = Tensor::empty(root.shape(), root.dtype(), Device::cpu);
    auto *ptr = static_cast<float *>(ones.data_ptr());
    std::size_t n = root.numel();
    for (std::size_t i = 0; i < n; ++i)
      ptr[i] = 1.0f;
    if (root.device() == Device::mps)
      g = ones.to(Device::mps);
    else
      g = ones;
  }
  if (auto fn = root.grad_fn())
    fn->apply(g);
  else
    root.set_grad(g);
}

} // namespace orchard::core::autograd
