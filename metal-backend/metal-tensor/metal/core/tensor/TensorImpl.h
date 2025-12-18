#pragma once

#include <array>
#include <cstddef>
#ifdef __APPLE__
#include <os/lock.h>
struct UnfairLock {
  os_unfair_lock l = OS_UNFAIR_LOCK_INIT;
  void lock() { os_unfair_lock_lock(&l); }
  void unlock() { os_unfair_lock_unlock(&l); }
};
using TensorLock = UnfairLock;
#else
#include <mutex>
using TensorLock = std::mutex;
#endif

#include "Storage.h"

namespace orchard::core::tensor {

struct TensorImpl {
  Storage *storage{nullptr};
  std::array<std::int64_t, 8> shape{};
  std::array<std::int64_t, 8> strides{};
  DType dtype{DType::f32};
  Device device{Device::cpu};
  std::int64_t offset{0};
  TensorLock lock;
  void *grad_fn{nullptr};
  void *grad_ctx{nullptr};

  TensorImpl() = default;
  TensorImpl(const TensorImpl &other)
      : storage(other.storage), shape(other.shape), strides(other.strides),
        dtype(other.dtype), device(other.device), offset(other.offset),
        grad_fn(other.grad_fn), grad_ctx(other.grad_ctx) {}
};

} // namespace orchard::core::tensor
