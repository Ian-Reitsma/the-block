#include "Tensor.h"
#include "../../runtime/CpuContext.h"
#include "../../runtime/MetalKernels.h"
#include "../../runtime/Runtime.h"
#include "../autograd/AddBackward.h"
#include "../autograd/DivBackward.h"
#include "../autograd/DivScalarBackward.h"
#include "../autograd/MatmulBackward.h"
#include "../autograd/MeanBackward.h"
#include "../autograd/MulBackward.h"
#include "../autograd/Node.h"
#include "../autograd/SumBackward.h"
#include "../autograd/TransposeBackward.h"
#include "../autograd/ViewBackward.h"

#include <algorithm>
#include <cassert>
#include <cstdint>
#include <cstring>
#include <mutex>
#include <sstream>
#include <stdexcept>

namespace orchard::core::tensor {

namespace {

int rank_of(const std::array<std::int64_t, 8> &shape) {
  int r = 0;
  for (auto s : shape) {
    if (s <= 0)
      break;
    ++r;
  }
  return r;
}

std::array<std::int64_t, 8>
contiguous_strides(const std::array<std::int64_t, 8> &shape) {
  std::array<std::int64_t, 8> strides{};
  int r = rank_of(shape);
  std::int64_t stride = 1;
  for (int i = r - 1; i >= 0; --i) {
    strides[i] = stride;
    stride *= shape[i];
  }
  return strides;
}

std::int64_t numel(const std::array<std::int64_t, 8> &shape) {
  int r = rank_of(shape);
  std::int64_t n = 1;
  for (int i = 0; i < r; ++i)
    n *= shape[i];
  return n;
}

bool aligned64(const void *ptr) {
  return reinterpret_cast<std::uintptr_t>(ptr) % 64 == 0;
}

struct BroadcastInfo {
  std::array<std::int64_t, 8> shape{};
  std::array<std::int64_t, 8> a_strides{};
  std::array<std::int64_t, 8> b_strides{};
};

bool compute_broadcast(const std::array<std::int64_t, 8> &a_shape,
                       const std::array<std::int64_t, 8> &a_strides,
                       const std::array<std::int64_t, 8> &b_shape,
                       const std::array<std::int64_t, 8> &b_strides,
                       BroadcastInfo &info) {
  for (int i = 7; i >= 0; --i) {
    std::int64_t as = a_shape[i];
    std::int64_t bs = b_shape[i];
    if (as == bs) {
      info.shape[i] = as;
      info.a_strides[i] = (as == 1) ? 0 : a_strides[i];
      info.b_strides[i] = (bs == 1) ? 0 : b_strides[i];
    } else if (as == 1) {
      info.shape[i] = bs;
      info.a_strides[i] = 0;
      info.b_strides[i] = b_strides[i];
    } else if (bs == 1) {
      info.shape[i] = as;
      info.a_strides[i] = a_strides[i];
      info.b_strides[i] = 0;
    } else {
      return false;
    }
  }
  return true;
}

template <bool Safe, typename F>
void cpu_broadcast_binary(const float *a, std::int64_t a_off,
                          const std::array<std::int64_t, 8> &astrides,
                          const float *b, std::int64_t b_off,
                          const std::array<std::int64_t, 8> &bstrides,
                          float *out, const std::array<std::int64_t, 8> &shape,
                          F fn) {
  std::size_t n = core::tensor::numel(shape);
  std::array<std::int64_t, 8> idx{};
  for (std::size_t i = 0; i < n; ++i) {
    std::int64_t ao = a_off;
    std::int64_t bo = b_off;
    for (int d = 0; d < 8; ++d) {
      ao += idx[d] * astrides[d];
      bo += idx[d] * bstrides[d];
    }
    float bv = b[bo];
    if constexpr (Safe) {
      out[i] = (bv == 0.0f) ? 0.0f : fn(a[ao], bv);
    } else {
      out[i] = fn(a[ao], bv);
    }
    for (int d = 7; d >= 0; --d) {
      idx[d]++;
      if (idx[d] < shape[d])
        break;
      idx[d] = 0;
    }
  }
}

} // namespace

Tensor::Tensor(const Tensor &other) {
  if (other.impl_) {
    impl_ = new TensorImpl(*other.impl_);
    if (impl_->storage) {
      std::lock_guard<TensorLock> guard(other.impl_->lock);
      impl_->storage->retain();
    }
  }
  requires_grad_ = other.requires_grad_;
  if (other.grad_)
    grad_ = std::make_unique<Tensor>(*other.grad_);
  grad_fn_ = other.grad_fn_;
}

Tensor &Tensor::operator=(const Tensor &other) {
  if (this == &other)
    return *this;
  if (impl_) {
    if (impl_->storage) {
      std::lock_guard<TensorLock> guard(impl_->lock);
      impl_->storage->release();
    }
    delete impl_;
  }
  impl_ = nullptr;
  if (other.impl_) {
    impl_ = new TensorImpl(*other.impl_);
    if (impl_->storage) {
      std::lock_guard<TensorLock> guard(other.impl_->lock);
      impl_->storage->retain();
    }
  }
  requires_grad_ = other.requires_grad_;
  if (other.grad_)
    grad_ = std::make_unique<Tensor>(*other.grad_);
  else
    grad_.reset();
  grad_fn_ = other.grad_fn_;
  return *this;
}

Tensor::Tensor(Tensor &&other) noexcept
    : impl_(other.impl_), requires_grad_(other.requires_grad_),
      grad_(std::move(other.grad_)), grad_fn_(std::move(other.grad_fn_)) {
  other.impl_ = nullptr;
  other.requires_grad_ = false;
  other.grad_.reset();
  other.grad_fn_.reset();
}

Tensor &Tensor::operator=(Tensor &&other) noexcept {
  if (this != &other) {
    if (impl_ && impl_->storage) {
      std::lock_guard<TensorLock> guard(impl_->lock);
      impl_->storage->release();
      delete impl_;
    }
    impl_ = other.impl_;
    requires_grad_ = other.requires_grad_;
    grad_ = std::move(other.grad_);
    grad_fn_ = std::move(other.grad_fn_);
    other.impl_ = nullptr;
    other.requires_grad_ = false;
    other.grad_.reset();
    other.grad_fn_.reset();
  }
  return *this;
}

Tensor::~Tensor() {
  if (impl_) {
    if (impl_->storage) {
      std::lock_guard<TensorLock> guard(impl_->lock);
      impl_->storage->release();
    }
    delete impl_;
  }
}

Tensor Tensor::empty(const std::array<std::int64_t, 8> &shape, DType dtype,
                     Device dev) {
  int r = rank_of(shape);
  if (r > 8)
    return Tensor{};
  std::int64_t n = core::tensor::numel(shape);
  std::size_t bytes = n * dtype_size(dtype);
  Storage *storage = Storage::create(bytes, dev);
  if (!storage)
    return Tensor{};
  auto *impl = new TensorImpl{};
  impl->storage = storage;
  impl->shape = shape;
  impl->strides = contiguous_strides(shape);
  impl->dtype = dtype;
  impl->device = dev;
  impl->offset = 0;
  return Tensor(impl);
}

Tensor Tensor::zerosLike(const Tensor &other) {
  Tensor t = empty(other.shape(), other.dtype(), other.device());
  if (t.impl_ && t.impl_->storage) {
    std::memset(t.impl_->storage->data, 0, t.impl_->storage->nbytes);
  }
  return t;
}

Tensor Tensor::fromData(void *data, const std::array<std::int64_t, 8> &shape,
                        DType dtype, Device dev,
                        std::function<void(void *)> deleter) {
  int r = rank_of(shape);
  if (!data || r > 8)
    return Tensor{};
  if (!aligned64(data))
    return Tensor{};
  std::size_t bytes = core::tensor::numel(shape) * dtype_size(dtype);
  Storage *storage = Storage::wrap(data, bytes, dev, std::move(deleter));
  if (!storage)
    return Tensor{};
  auto *impl = new TensorImpl{};
  impl->storage = storage;
  impl->shape = shape;
  impl->strides = contiguous_strides(shape);
  impl->dtype = dtype;
  impl->device = dev;
  impl->offset = 0;
  return Tensor(impl);
}

Tensor Tensor::view(const std::array<std::int64_t, 8> &newShape) const {
  int r = rank_of(newShape);
  if (r > 8 || !impl_ || !impl_->storage)
    return Tensor{};
  for (int i = 0; i < r; ++i) {
    if (newShape[i] <= 0)
      return Tensor{};
  }
  if (core::tensor::numel(newShape) != core::tensor::numel(impl_->shape))
    return Tensor{};
  auto *impl = new TensorImpl{};
  {
    std::lock_guard<TensorLock> guard(this->impl_->lock);
    this->impl_->storage->retain();
  }
  impl->storage = this->impl_->storage;
  impl->dtype = this->impl_->dtype;
  impl->device = this->impl_->device;
  impl->shape = newShape;
  impl->strides = contiguous_strides(newShape);
  impl->offset = this->impl_->offset;
  Tensor t(impl);
  t.set_requires_grad(requires_grad_);
  if (requires_grad_) {
    t.set_grad_fn(std::make_shared<autograd::ViewBackward>(*this));
  } else {
    t.set_grad_fn(grad_fn_);
  }
  return t;
}

Tensor Tensor::transpose(int dim0, int dim1) const {
  if (!impl_ || !impl_->storage)
    return Tensor{};
  int r = rank_of(impl_->shape);
  if (dim0 < 0 || dim1 < 0 || dim0 >= r || dim1 >= r)
    return Tensor{};
  auto *impl = new TensorImpl{};
  {
    std::lock_guard<TensorLock> guard(this->impl_->lock);
    this->impl_->storage->retain();
  }
  impl->storage = this->impl_->storage;
  impl->dtype = this->impl_->dtype;
  impl->device = this->impl_->device;
  impl->shape = this->impl_->shape;
  impl->strides = this->impl_->strides;
  std::swap(impl->shape[dim0], impl->shape[dim1]);
  std::swap(impl->strides[dim0], impl->strides[dim1]);
  impl->offset = this->impl_->offset;
  Tensor t(impl);
  t.set_requires_grad(requires_grad_);
  if (requires_grad_) {
    t.set_grad_fn(
        std::make_shared<autograd::TransposeBackward>(*this, dim0, dim1));
  } else {
    t.set_grad_fn(grad_fn_);
  }
  return t;
}

Tensor Tensor::slice(int dim, int start, int end, int step) const {
  if (!impl_ || !impl_->storage)
    return Tensor{};
  int r = rank_of(impl_->shape);
  if (dim < 0 || dim >= r || step <= 0)
    return Tensor{};
  if (start < 0 || end > impl_->shape[dim] || start >= end)
    return Tensor{};

  auto newShape = impl_->shape;
  auto newStrides = impl_->strides;
  std::int64_t len = (end - start + step - 1) / step;
  newShape[dim] = len;
  newStrides[dim] *= step;

  auto *impl = new TensorImpl{};
  {
    std::lock_guard<TensorLock> guard(this->impl_->lock);
    this->impl_->storage->retain();
  }
  impl->storage = this->impl_->storage;
  impl->dtype = this->impl_->dtype;
  impl->device = this->impl_->device;
  impl->shape = newShape;
  impl->strides = newStrides;
  impl->offset = this->impl_->offset + start * this->impl_->strides[dim];
  Tensor t(impl);
  t.set_requires_grad(requires_grad_);
  t.set_grad_fn(grad_fn_);
  return t;
}

Tensor Tensor::to(Device dev) const {
  if (!impl_ || !impl_->storage)
    return Tensor{};
  if (dev == impl_->device) {
    auto *impl = new TensorImpl{};
    {
      std::lock_guard<TensorLock> guard(this->impl_->lock);
      this->impl_->storage->retain();
    }
    impl->storage = this->impl_->storage;
    impl->dtype = this->impl_->dtype;
    impl->device = this->impl_->device;
    impl->shape = this->impl_->shape;
    impl->strides = this->impl_->strides;
    impl->offset = this->impl_->offset;
    Tensor t(impl);
    t.set_requires_grad(requires_grad_);
    t.set_grad_fn(grad_fn_);
    return t;
  }

  Tensor t = empty(impl_->shape, impl_->dtype, dev);
  if (t.impl_ && t.impl_->storage) {
    Tensor src = contiguous();
    if (src.impl_ && src.impl_->storage) {
      std::size_t bytes = t.impl_->storage->nbytes;
      if (impl_->device == Device::cpu && dev == Device::cpu) {
        std::memcpy(t.data_ptr(), src.data_ptr(), bytes);
      }
#ifdef __APPLE__
      else if (impl_->device == Device::cpu && dev == Device::mps) {
        if (!aligned64(src.data_ptr()))
          return Tensor{};
        orchard::runtime::metal_copy_cpu_to_metal(
            static_cast<orchard::runtime::MTLBufferRef>(t.impl_->storage->data),
            src.data_ptr(), bytes);
      } else if (impl_->device == Device::mps && dev == Device::cpu) {
        if (!aligned64(t.data_ptr()))
          return Tensor{};
        orchard::runtime::metal_copy_metal_to_cpu(
            t.data_ptr(),
            static_cast<orchard::runtime::MTLBufferRef>(
                src.impl_->storage->data),
            bytes);
      } else if (impl_->device == Device::mps && dev == Device::mps) {
        orchard::runtime::metal_copy_buffers(
            static_cast<orchard::runtime::MTLBufferRef>(t.impl_->storage->data),
            static_cast<orchard::runtime::MTLBufferRef>(
                src.impl_->storage->data),
            bytes);
      } else {
        std::memcpy(t.data_ptr(), src.data_ptr(), bytes);
      }
#else
      else {
        std::memcpy(t.data_ptr(), src.data_ptr(), bytes);
      }
#endif
    }
  }
  t.set_requires_grad(requires_grad_);
  t.set_grad_fn(grad_fn_);
  return t;
}

Tensor Tensor::contiguous() const {
  if (!impl_ || !impl_->storage)
    return Tensor{};
  if (is_contiguous()) {
    auto *impl = new TensorImpl{};
    {
      std::lock_guard<TensorLock> guard(this->impl_->lock);
      this->impl_->storage->retain();
    }
    impl->storage = this->impl_->storage;
    impl->dtype = this->impl_->dtype;
    impl->device = this->impl_->device;
    impl->shape = this->impl_->shape;
    impl->strides = this->impl_->strides;
    impl->offset = this->impl_->offset;
    Tensor t(impl);
    t.set_requires_grad(requires_grad_);
    t.set_grad_fn(grad_fn_);
    return t;
  }

  Tensor out = empty(impl_->shape, impl_->dtype, impl_->device);
  int r = rank_of(impl_->shape);
  std::array<std::int64_t, 8> dstStrides = contiguous_strides(impl_->shape);
  std::size_t esize = dtype_size(impl_->dtype);

  const char *src = static_cast<const char *>(impl_->storage->data);
  char *dst = static_cast<char *>(out.impl_->storage->data);
  std::array<std::int64_t, 8> idx{};
  for (std::int64_t i0 = 0; i0 < (r > 0 ? impl_->shape[0] : 1); ++i0) {
    idx[0] = i0;
    for (std::int64_t i1 = 0; i1 < (r > 1 ? impl_->shape[1] : 1); ++i1) {
      idx[1] = i1;
      for (std::int64_t i2 = 0; i2 < (r > 2 ? impl_->shape[2] : 1); ++i2) {
        idx[2] = i2;
        for (std::int64_t i3 = 0; i3 < (r > 3 ? impl_->shape[3] : 1); ++i3) {
          idx[3] = i3;
          for (std::int64_t i4 = 0; i4 < (r > 4 ? impl_->shape[4] : 1); ++i4) {
            idx[4] = i4;
            for (std::int64_t i5 = 0; i5 < (r > 5 ? impl_->shape[5] : 1);
                 ++i5) {
              idx[5] = i5;
              for (std::int64_t i6 = 0; i6 < (r > 6 ? impl_->shape[6] : 1);
                   ++i6) {
                idx[6] = i6;
                for (std::int64_t i7 = 0; i7 < (r > 7 ? impl_->shape[7] : 1);
                     ++i7) {
                  idx[7] = i7;
                  std::int64_t srcOff = impl_->offset;
                  std::int64_t dstOff = 0;
                  for (int d = 0; d < r; ++d) {
                    srcOff += idx[d] * impl_->strides[d];
                    dstOff += idx[d] * dstStrides[d];
                  }
                  std::memcpy(dst + dstOff * esize, src + srcOff * esize,
                              esize);
                }
              }
            }
          }
        }
      }
    }
  }
  out.set_requires_grad(requires_grad_);
  out.set_grad_fn(grad_fn_);
  return out;
}

Tensor Tensor::add(const Tensor &other) const {
  if (!impl_ || !other.impl_)
    return Tensor{};
  BroadcastInfo info{};
  if (!compute_broadcast(impl_->shape, impl_->strides, other.impl_->shape,
                         other.impl_->strides, info))
    return Tensor{};
  Tensor out = empty(info.shape, impl_->dtype, impl_->device);
  std::size_t n = core::tensor::numel(info.shape);
  if (impl_->device == Device::cpu) {
    cpu_broadcast_binary<false>(
        static_cast<const float *>(impl_->storage->data), impl_->offset,
        info.a_strides, static_cast<const float *>(other.impl_->storage->data),
        other.impl_->offset, info.b_strides,
        static_cast<float *>(out.impl_->storage->data), info.shape,
        [](float x, float y) { return x + y; });
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_add(
          static_cast<const float *>(impl_->storage->data) + impl_->offset,
          static_cast<const float *>(other.impl_->storage->data) +
              other.impl_->offset,
          static_cast<float *>(out.impl_->storage->data) + out.impl_->offset,
          info.shape.data(), info.a_strides.data(), info.b_strides.data(),
          static_cast<std::uint32_t>(rank_of(info.shape)), n);
    } catch (const std::runtime_error &) {
      cpu_broadcast_binary<false>(
          static_cast<const float *>(impl_->storage->data), impl_->offset,
          info.a_strides,
          static_cast<const float *>(other.impl_->storage->data),
          other.impl_->offset, info.b_strides,
          static_cast<float *>(out.impl_->storage->data), info.shape,
          [](float x, float y) { return x + y; });
    }
  }
  out.set_requires_grad(requires_grad_ || other.requires_grad_);
  if (out.requires_grad())
    out.set_grad_fn(std::make_shared<autograd::AddBackward>(*this, other));
  return out;
}

Tensor Tensor::mul(const Tensor &other) const {
  if (!impl_ || !other.impl_)
    return Tensor{};
  BroadcastInfo info{};
  if (!compute_broadcast(impl_->shape, impl_->strides, other.impl_->shape,
                         other.impl_->strides, info))
    return Tensor{};
  Tensor out = empty(info.shape, impl_->dtype, impl_->device);
  std::size_t n = core::tensor::numel(info.shape);
  if (impl_->device == Device::cpu) {
    cpu_broadcast_binary<false>(
        static_cast<const float *>(impl_->storage->data), impl_->offset,
        info.a_strides, static_cast<const float *>(other.impl_->storage->data),
        other.impl_->offset, info.b_strides,
        static_cast<float *>(out.impl_->storage->data), info.shape,
        [](float x, float y) { return x * y; });
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_mul(
          static_cast<const float *>(impl_->storage->data) + impl_->offset,
          static_cast<const float *>(other.impl_->storage->data) +
              other.impl_->offset,
          static_cast<float *>(out.impl_->storage->data) + out.impl_->offset,
          info.shape.data(), info.a_strides.data(), info.b_strides.data(),
          static_cast<std::uint32_t>(rank_of(info.shape)), n);
    } catch (const std::runtime_error &) {
      cpu_broadcast_binary<false>(
          static_cast<const float *>(impl_->storage->data), impl_->offset,
          info.a_strides,
          static_cast<const float *>(other.impl_->storage->data),
          other.impl_->offset, info.b_strides,
          static_cast<float *>(out.impl_->storage->data), info.shape,
          [](float x, float y) { return x * y; });
    }
  }
  bool rg = requires_grad_ || other.requires_grad_;
  out.set_requires_grad(rg);
  if (rg)
    out.set_grad_fn(std::make_shared<autograd::MulBackward>(*this, other));
  return out;
}

Tensor Tensor::div(const Tensor &other, bool safe) const {
  if (!impl_ || !other.impl_)
    return Tensor{};
  Tensor other_cpu;
  if (!safe) {
    other_cpu = other.to(Device::cpu);
    auto *bp = static_cast<const float *>(other_cpu.data_ptr());
    std::size_t dn = other_cpu.numel();
    bool has_zero = false;
    for (std::size_t i = 0; i < dn; ++i) {
      if (bp[i] == 0.0f) {
        has_zero = true;
        break;
      }
    }
    if (has_zero)
      throw std::runtime_error("division by zero");
  }
  BroadcastInfo info{};
  if (!compute_broadcast(impl_->shape, impl_->strides, other.impl_->shape,
                         other.impl_->strides, info))
    return Tensor{};
  Tensor out = empty(info.shape, impl_->dtype, impl_->device);
  std::size_t n = core::tensor::numel(info.shape);
  if (impl_->device == Device::cpu) {
    Tensor ob = safe ? other.to(Device::cpu) : other_cpu;
    if (safe) {
      cpu_broadcast_binary<true>(
          static_cast<const float *>(impl_->storage->data), impl_->offset,
          info.a_strides, static_cast<const float *>(ob.impl_->storage->data),
          ob.impl_->offset, info.b_strides,
          static_cast<float *>(out.impl_->storage->data), info.shape,
          [](float x, float y) { return x / y; });
    } else {
      cpu_broadcast_binary<false>(
          static_cast<const float *>(impl_->storage->data), impl_->offset,
          info.a_strides, static_cast<const float *>(ob.impl_->storage->data),
          ob.impl_->offset, info.b_strides,
          static_cast<float *>(out.impl_->storage->data), info.shape,
          [](float x, float y) { return x / y; });
    }
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_div(
          static_cast<const float *>(impl_->storage->data) + impl_->offset,
          static_cast<const float *>(other.impl_->storage->data) +
              other.impl_->offset,
          static_cast<float *>(out.impl_->storage->data) + out.impl_->offset,
          info.shape.data(), info.a_strides.data(), info.b_strides.data(),
          static_cast<std::uint32_t>(rank_of(info.shape)), n, safe);
    } catch (const std::runtime_error &) {
      Tensor ob = safe ? other.to(Device::cpu) : other_cpu;
      if (safe) {
        cpu_broadcast_binary<true>(
            static_cast<const float *>(impl_->storage->data), impl_->offset,
            info.a_strides, static_cast<const float *>(ob.impl_->storage->data),
            ob.impl_->offset, info.b_strides,
            static_cast<float *>(out.impl_->storage->data), info.shape,
            [](float x, float y) { return x / y; });
      } else {
        cpu_broadcast_binary<false>(
            static_cast<const float *>(impl_->storage->data), impl_->offset,
            info.a_strides, static_cast<const float *>(ob.impl_->storage->data),
            ob.impl_->offset, info.b_strides,
            static_cast<float *>(out.impl_->storage->data), info.shape,
            [](float x, float y) { return x / y; });
      }
    }
  }
  bool rg = requires_grad_ || other.requires_grad_;
  out.set_requires_grad(rg);
  if (rg)
    out.set_grad_fn(
        std::make_shared<autograd::DivBackward>(*this, other, safe));
  return out;
}

Tensor Tensor::div(float scalar, bool safe) const {
  if (!impl_)
    return Tensor{};
  if (scalar == 0.0f && !safe)
    throw std::runtime_error("division by zero");
  Tensor out = empty(impl_->shape, impl_->dtype, impl_->device);
  std::size_t n = numel();
  if (impl_->device == Device::cpu) {
    auto *ap = static_cast<const float *>(data_ptr());
    auto *op = static_cast<float *>(out.data_ptr());
    if (safe && scalar == 0.0f) {
      for (std::size_t i = 0; i < n; ++i)
        op[i] = 0.0f;
    } else {
      for (std::size_t i = 0; i < n; ++i)
        op[i] = ap[i] / scalar;
    }
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_div_scalar(
          static_cast<const float *>(impl_->storage->data) + impl_->offset,
          scalar,
          static_cast<float *>(out.impl_->storage->data) + out.impl_->offset, n,
          safe);
    } catch (const std::runtime_error &) {
      auto *ap = static_cast<const float *>(data_ptr());
      auto *op = static_cast<float *>(out.data_ptr());
      if (safe && scalar == 0.0f) {
        for (std::size_t i = 0; i < n; ++i)
          op[i] = 0.0f;
      } else {
        for (std::size_t i = 0; i < n; ++i)
          op[i] = ap[i] / scalar;
      }
    }
  }
  out.set_requires_grad(requires_grad_);
  if (requires_grad_) {
    Tensor before = clone();
    auto &self = const_cast<Tensor &>(*this);
    out.set_grad_fn(std::make_shared<autograd::DivScalarBackward>(
        before, self, scalar, safe));
  }
  return out;
}

Tensor &Tensor::div_(float scalar, bool safe) {
  if (!impl_)
    return *this;
  if (scalar == 0.0f && !safe)
    throw std::runtime_error("division by zero");
  Tensor before;
  if (requires_grad_) {
    before = clone().detach();
    before.set_requires_grad(true);
    before.set_grad_fn(grad_fn_);
  }
  std::size_t n = numel();
  if (impl_->device == Device::cpu) {
    auto *ap = static_cast<float *>(data_ptr());
    if (safe && scalar == 0.0f) {
      for (std::size_t i = 0; i < n; ++i)
        ap[i] = 0.0f;
    } else {
      for (std::size_t i = 0; i < n; ++i)
        ap[i] /= scalar;
    }
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_div_scalar(
          static_cast<const float *>(impl_->storage->data) + impl_->offset,
          scalar, static_cast<float *>(impl_->storage->data) + impl_->offset, n,
          safe);
    } catch (const std::runtime_error &) {
      auto *ap = static_cast<float *>(data_ptr());
      if (safe && scalar == 0.0f) {
        for (std::size_t i = 0; i < n; ++i)
          ap[i] = 0.0f;
      } else {
        for (std::size_t i = 0; i < n; ++i)
          ap[i] /= scalar;
      }
    }
  }
  if (requires_grad_)
    set_grad_fn(std::make_shared<autograd::DivScalarBackward>(before, *this,
                                                              scalar, safe));
  return *this;
}

Tensor Tensor::matmul(const Tensor &other) const {
  if (!impl_ || !other.impl_)
    return Tensor{};
  std::int64_t m = impl_->shape[0];
  std::int64_t k = impl_->shape[1];
  std::int64_t n = other.impl_->shape[1];
  std::array<std::int64_t, 8> outShape{m, n, 1, 1, 1, 1, 1, 1};
  Tensor out = empty(outShape, impl_->dtype, impl_->device);
  if (impl_->device == Device::cpu) {
    auto *ap = static_cast<const float *>(data_ptr());
    auto *bp = static_cast<const float *>(other.data_ptr());
    auto *cp = static_cast<float *>(out.data_ptr());
    for (std::int64_t i = 0; i < m; ++i) {
      for (std::int64_t j = 0; j < n; ++j) {
        float s = 0.0f;
        for (std::int64_t p = 0; p < k; ++p)
          s += ap[i * k + p] * bp[p * n + j];
        cp[i * n + j] = s;
      }
    }
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_matmul(
          static_cast<const float *>(impl_->storage->data),
          static_cast<const float *>(other.impl_->storage->data),
          static_cast<float *>(out.impl_->storage->data), m, n, k);
    } catch (const std::runtime_error &) {
      auto *ap = static_cast<const float *>(data_ptr());
      auto *bp = static_cast<const float *>(other.data_ptr());
      auto *cp = static_cast<float *>(out.data_ptr());
      for (std::int64_t i = 0; i < m; ++i) {
        for (std::int64_t j = 0; j < n; ++j) {
          float s = 0.0f;
          for (std::int64_t p = 0; p < k; ++p)
            s += ap[i * k + p] * bp[p * n + j];
          cp[i * n + j] = s;
        }
      }
    }
  }
  bool rg = requires_grad_ || other.requires_grad_;
  out.set_requires_grad(rg);
  if (rg) {
    out.set_grad_fn(std::make_shared<autograd::MatmulBackward>(*this, other));
  }
  return out;
}

Tensor Tensor::sum() const {
  if (!impl_)
    return Tensor{};
  Tensor out = empty({1, 1, 1, 1, 1, 1, 1, 1}, impl_->dtype, impl_->device);
  if (impl_->device == Device::cpu) {
    float s = 0.0f;
    auto *ap = static_cast<const float *>(data_ptr());
    for (std::size_t i = 0; i < numel(); ++i)
      s += ap[i];
    *static_cast<float *>(out.data_ptr()) = s;
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_reduce_sum(
          static_cast<const float *>(impl_->storage->data),
          static_cast<float *>(out.impl_->storage->data), numel());
    } catch (const std::runtime_error &) {
      float s = 0.0f;
      auto *ap = static_cast<const float *>(data_ptr());
      for (std::size_t i = 0; i < numel(); ++i)
        s += ap[i];
      *static_cast<float *>(out.data_ptr()) = s;
    }
  }
  out.set_requires_grad(requires_grad_);
  if (requires_grad_) {
    out.set_grad_fn(std::make_shared<autograd::SumBackward>(*this));
  }
  return out;
}

Tensor Tensor::mean() const {
  if (!impl_)
    return Tensor{};
  Tensor out = empty({1, 1, 1, 1, 1, 1, 1, 1}, impl_->dtype, impl_->device);
  if (impl_->device == Device::cpu) {
    float s = 0.0f;
    auto *ap = static_cast<const float *>(data_ptr());
    for (std::size_t i = 0; i < numel(); ++i)
      s += ap[i];
    *static_cast<float *>(out.data_ptr()) = s / static_cast<float>(numel());
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_mean(static_cast<const float *>(impl_->storage->data),
                          static_cast<float *>(out.impl_->storage->data),
                          numel());
    } catch (const std::runtime_error &) {
      float s = 0.0f;
      auto *ap = static_cast<const float *>(data_ptr());
      for (std::size_t i = 0; i < numel(); ++i)
        s += ap[i];
      *static_cast<float *>(out.data_ptr()) = s / static_cast<float>(numel());
    }
  }
  out.set_requires_grad(requires_grad_);
  if (requires_grad_) {
    out.set_grad_fn(std::make_shared<autograd::MeanBackward>(*this));
  }
  return out;
}

Tensor Tensor::sum(int dim, bool keepdim) const {
  if (!impl_)
    return Tensor{};
  int r = rank_of(impl_->shape);
  if (dim < 0)
    dim += r;
  std::array<std::int64_t, 8> outShape = impl_->shape;
  std::array<std::int64_t, 8> outStrides = impl_->strides;
  std::int64_t axisLen = impl_->shape[dim];
  if (keepdim) {
    outShape[dim] = 1;
    outStrides[dim] = 0;
  } else {
    for (int i = dim; i < 7; ++i) {
      outShape[i] = outShape[i + 1];
      outStrides[i] = outStrides[i + 1];
    }
    outShape[7] = 1;
    outStrides[7] = 0;
  }
  Tensor out = empty(outShape, impl_->dtype, impl_->device);
  if (impl_->device == Device::cpu) {
    auto *ap = static_cast<const float *>(data_ptr());
    auto *op = static_cast<float *>(out.data_ptr());
    int r_out = keepdim ? r : r - 1;
    std::size_t n = 1;
    for (int i = 0; i < r_out; ++i)
      n *= outShape[i];
    for (std::size_t i = 0; i < n; ++i) {
      std::size_t idx = i;
      std::int64_t base = impl_->offset;
      for (int d = r_out - 1; d >= 0; --d) {
        std::int64_t s = outShape[d];
        std::int64_t coord = idx % s;
        idx /= s;
        base += coord * outStrides[d];
      }
      float s = 0.0f;
      std::int64_t pos = base;
      for (std::int64_t j = 0; j < axisLen; ++j) {
        s += ap[pos];
        pos += impl_->strides[dim];
      }
      op[i] = s;
    }
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_reduce_sum_axis(
          static_cast<const float *>(impl_->storage->data),
          static_cast<float *>(out.impl_->storage->data), outShape.data(),
          impl_->strides.data(), static_cast<std::uint32_t>(rank_of(outShape)),
          static_cast<std::uint32_t>(axisLen), static_cast<std::uint32_t>(dim),
          static_cast<std::size_t>(core::tensor::numel(outShape)));
    } catch (const std::runtime_error &) {
      auto *ap = static_cast<const float *>(data_ptr());
      auto *op = static_cast<float *>(out.data_ptr());
      int r_out = keepdim ? r : r - 1;
      std::size_t n = 1;
      for (int i = 0; i < r_out; ++i)
        n *= outShape[i];
      for (std::size_t i = 0; i < n; ++i) {
        std::size_t idx = i;
        std::int64_t base = impl_->offset;
        for (int d = r_out - 1; d >= 0; --d) {
          std::int64_t s = outShape[d];
          std::int64_t coord = idx % s;
          idx /= s;
          base += coord * outStrides[d];
        }
        float s = 0.0f;
        std::int64_t pos = base;
        for (std::int64_t j = 0; j < axisLen; ++j) {
          s += ap[pos];
          pos += impl_->strides[dim];
        }
        op[i] = s;
      }
    }
  }
  out.set_requires_grad(requires_grad_);
  if (requires_grad_)
    out.set_grad_fn(
        std::make_shared<autograd::SumBackward>(*this, dim, keepdim));
  return out;
}

Tensor Tensor::mean(int dim, bool keepdim) const {
  if (!impl_)
    return Tensor{};
  int r = rank_of(impl_->shape);
  if (dim < 0)
    dim += r;
  std::array<std::int64_t, 8> outShape = impl_->shape;
  std::array<std::int64_t, 8> outStrides = impl_->strides;
  std::int64_t axisLen = impl_->shape[dim];
  if (keepdim) {
    outShape[dim] = 1;
    outStrides[dim] = 0;
  } else {
    for (int i = dim; i < 7; ++i) {
      outShape[i] = outShape[i + 1];
      outStrides[i] = outStrides[i + 1];
    }
    outShape[7] = 1;
    outStrides[7] = 0;
  }
  Tensor out = empty(outShape, impl_->dtype, impl_->device);
  if (impl_->device == Device::cpu) {
    auto *ap = static_cast<const float *>(data_ptr());
    auto *op = static_cast<float *>(out.data_ptr());
    int r_out = keepdim ? r : r - 1;
    std::size_t n = 1;
    for (int i = 0; i < r_out; ++i)
      n *= outShape[i];
    for (std::size_t i = 0; i < n; ++i) {
      std::size_t idx = i;
      std::int64_t base = impl_->offset;
      for (int d = r_out - 1; d >= 0; --d) {
        std::int64_t s = outShape[d];
        std::int64_t coord = idx % s;
        idx /= s;
        base += coord * outStrides[d];
      }
      float s = 0.0f;
      std::int64_t pos = base;
      for (std::int64_t j = 0; j < axisLen; ++j) {
        s += ap[pos];
        pos += impl_->strides[dim];
      }
      op[i] = s / static_cast<float>(axisLen);
    }
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_mean_axis(
          static_cast<const float *>(impl_->storage->data),
          static_cast<float *>(out.impl_->storage->data), outShape.data(),
          impl_->strides.data(), static_cast<std::uint32_t>(rank_of(outShape)),
          static_cast<std::uint32_t>(axisLen), static_cast<std::uint32_t>(dim),
          static_cast<std::size_t>(core::tensor::numel(outShape)));
    } catch (const std::runtime_error &) {
      auto *ap = static_cast<const float *>(data_ptr());
      auto *op = static_cast<float *>(out.data_ptr());
      int r_out = keepdim ? r : r - 1;
      std::size_t n = 1;
      for (int i = 0; i < r_out; ++i)
        n *= outShape[i];
      for (std::size_t i = 0; i < n; ++i) {
        std::size_t idx = i;
        std::int64_t base = impl_->offset;
        for (int d = r_out - 1; d >= 0; --d) {
          std::int64_t s = outShape[d];
          std::int64_t coord = idx % s;
          idx /= s;
          base += coord * outStrides[d];
        }
        float s = 0.0f;
        std::int64_t pos = base;
        for (std::int64_t j = 0; j < axisLen; ++j) {
          s += ap[pos];
          pos += impl_->strides[dim];
        }
        op[i] = s / static_cast<float>(axisLen);
      }
    }
  }
  out.set_requires_grad(requires_grad_);
  if (requires_grad_)
    out.set_grad_fn(
        std::make_shared<autograd::MeanBackward>(*this, dim, keepdim));
  return out;
}

void Tensor::fill(float value) {
  if (!impl_ || !impl_->storage)
    return;
  std::size_t n = numel();
  if (impl_->device == Device::cpu) {
    auto *p = static_cast<float *>(data_ptr());
    for (std::size_t i = 0; i < n; ++i)
      p[i] = value;
  } else if (impl_->device == Device::mps) {
    try {
      runtime::metal_fill(static_cast<float *>(impl_->storage->data), value, n);
    } catch (const std::runtime_error &) {
      auto *p = static_cast<float *>(data_ptr());
      for (std::size_t i = 0; i < n; ++i)
        p[i] = value;
    }
  }
}

std::size_t Tensor::numel() const {
  return impl_ ? core::tensor::numel(impl_->shape) : 0;
}

bool Tensor::is_contiguous() const {
  if (!impl_)
    return true;
  std::array<std::int64_t, 8> expected = contiguous_strides(impl_->shape);
  int r = rank_of(impl_->shape);
  for (int i = 0; i < r; ++i) {
    if (impl_->strides[i] != expected[i])
      return false;
  }
  return true;
}

Tensor Tensor::clone() const {
  if (!impl_ || !impl_->storage)
    return Tensor{};
  Tensor out = empty(impl_->shape, impl_->dtype, impl_->device);
  int r = rank_of(impl_->shape);
  std::array<std::int64_t, 8> dstStrides = contiguous_strides(impl_->shape);
  std::size_t esize = dtype_size(impl_->dtype);
  const char *src = static_cast<const char *>(impl_->storage->data);
  char *dst = static_cast<char *>(out.impl_->storage->data);
  std::array<std::int64_t, 8> idx{};
  for (std::int64_t i0 = 0; i0 < (r > 0 ? impl_->shape[0] : 1); ++i0) {
    idx[0] = i0;
    for (std::int64_t i1 = 0; i1 < (r > 1 ? impl_->shape[1] : 1); ++i1) {
      idx[1] = i1;
      for (std::int64_t i2 = 0; i2 < (r > 2 ? impl_->shape[2] : 1); ++i2) {
        idx[2] = i2;
        for (std::int64_t i3 = 0; i3 < (r > 3 ? impl_->shape[3] : 1); ++i3) {
          idx[3] = i3;
          for (std::int64_t i4 = 0; i4 < (r > 4 ? impl_->shape[4] : 1); ++i4) {
            idx[4] = i4;
            for (std::int64_t i5 = 0; i5 < (r > 5 ? impl_->shape[5] : 1);
                 ++i5) {
              idx[5] = i5;
              for (std::int64_t i6 = 0; i6 < (r > 6 ? impl_->shape[6] : 1);
                   ++i6) {
                idx[6] = i6;
                for (std::int64_t i7 = 0; i7 < (r > 7 ? impl_->shape[7] : 1);
                     ++i7) {
                  idx[7] = i7;
                  std::int64_t srcOff = impl_->offset;
                  std::int64_t dstOff = 0;
                  for (int d = 0; d < r; ++d) {
                    srcOff += idx[d] * impl_->strides[d];
                    dstOff += idx[d] * dstStrides[d];
                  }
                  std::memcpy(dst + dstOff * esize, src + srcOff * esize,
                              esize);
                }
              }
            }
          }
        }
      }
    }
  }
  out.set_requires_grad(requires_grad_);
  out.set_grad_fn(grad_fn_);
  return out;
}

Tensor Tensor::detach() const {
  Tensor out(*this);
  out.set_requires_grad(false);
  out.set_grad_fn(nullptr);
  out.set_grad(Tensor{});
  return out;
}

bool Tensor::is_alias_of(const Tensor &other) const {
  return impl_ && other.impl_ && impl_->storage == other.impl_->storage;
}

std::string Tensor::toString() const {
  if (!impl_)
    return "Tensor()";
  std::ostringstream oss;
  oss << "Tensor(dtype=" << static_cast<int>(impl_->dtype)
      << ", device=" << device_name(impl_->device) << ", shape=[";
  int r = rank_of(impl_->shape);
  for (int i = 0; i < r; ++i) {
    if (i)
      oss << ", ";
    oss << impl_->shape[i];
  }
  oss << "], strides=[";
  for (int i = 0; i < r; ++i) {
    if (i)
      oss << ", ";
    oss << impl_->strides[i];
  }
  oss << "])";
  return oss.str();
}

void Tensor::backward() const {
  autograd::backward(const_cast<Tensor &>(*this));
}

} // namespace orchard::core::tensor
