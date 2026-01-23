#pragma once

#include <array>
#include <functional>
#include <memory>
#include <string>

#include "TensorImpl.h"

namespace orchard::core {
namespace autograd {
struct Node;
}
namespace tensor {

class Tensor {
public:
  Tensor() = default;
  explicit Tensor(TensorImpl *impl) : impl_(impl) {}

  Tensor(const Tensor &other);
  Tensor &operator=(const Tensor &other);
  Tensor(Tensor &&other) noexcept;
  Tensor &operator=(Tensor &&other) noexcept;
  ~Tensor();

  [[nodiscard]] static Tensor empty(const std::array<std::int64_t, 8> &shape,
                                    DType dtype, Device dev);
  [[nodiscard]] static Tensor zerosLike(const Tensor &other);
  [[nodiscard]] static Tensor
  fromData(void *data, const std::array<std::int64_t, 8> &shape, DType dtype,
           Device dev, std::function<void(void *)> deleter = nullptr);
  [[nodiscard]] Tensor view(const std::array<std::int64_t, 8> &newShape) const;
  [[nodiscard]] Tensor transpose(int dim0, int dim1) const;
  [[nodiscard]] Tensor slice(int dim, int start, int end, int step = 1) const;
  [[nodiscard]] Tensor to(Device dev) const;
  [[nodiscard]] Tensor contiguous() const;
  [[nodiscard]] Tensor add(const Tensor &other) const;
  [[nodiscard]] Tensor mul(const Tensor &other) const;
  [[nodiscard]] Tensor div(const Tensor &other, bool safe = false) const;
  [[nodiscard]] Tensor div(float scalar, bool safe = false) const;
  Tensor &div_(float scalar, bool safe = false);
  [[nodiscard]] Tensor matmul(const Tensor &other) const;
  [[nodiscard]] Tensor sum() const;
  [[nodiscard]] Tensor mean() const;
  [[nodiscard]] Tensor sum(int dim, bool keepdim = false) const;
  [[nodiscard]] Tensor mean(int dim, bool keepdim = false) const;
  void fill(float value);
  void *data_ptr() const {
    if (!impl_ || !impl_->storage)
      return nullptr;
    auto *base = static_cast<char *>(impl_->storage->data);
    return base + impl_->offset * dtype_size(impl_->dtype);
  }
  std::int64_t offset() const { return impl_->offset; }
  [[nodiscard]] Tensor clone() const;
  [[nodiscard]] Tensor detach() const;
  [[nodiscard]] bool is_alias_of(const Tensor &other) const;

  DType dtype() const { return impl_->dtype; }
  Device device() const { return impl_->device; }
  const std::array<std::int64_t, 8> &shape() const { return impl_->shape; }
  const std::array<std::int64_t, 8> &strides() const { return impl_->strides; }
  std::size_t nbytes() const {
    return impl_->storage ? impl_->storage->nbytes : 0;
  }
  std::size_t numel() const;
  bool is_contiguous() const;
  std::string toString() const;
  void backward() const;

  bool requires_grad() const { return requires_grad_; }
  void set_requires_grad(bool v) { requires_grad_ = v; }
  const Tensor &grad() const {
    static Tensor empty_grad;
    return grad_ ? *grad_ : empty_grad;
  }
  Tensor &grad() {
    if (!grad_)
      grad_ = std::make_unique<Tensor>();
    return *grad_;
  }
  void set_grad(const Tensor &g) { grad() = g; }
  std::shared_ptr<autograd::Node> grad_fn() const { return grad_fn_; }
  void set_grad_fn(std::shared_ptr<autograd::Node> fn) {
    grad_fn_ = std::move(fn);
  }

private:
  TensorImpl *impl_{nullptr};
  bool requires_grad_{false};
  std::unique_ptr<Tensor> grad_{};
  std::shared_ptr<autograd::Node> grad_fn_{};
};

} // namespace tensor
} // namespace orchard::core
