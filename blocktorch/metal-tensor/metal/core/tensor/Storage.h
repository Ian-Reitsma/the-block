#pragma once

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <functional>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include <uuid/uuid.h>

#include "DType.h"
#include "common/Profiling.h"
#include "runtime/Allocator.h"

namespace orchard::core::tensor {

struct Storage;

inline std::mutex live_storage_mutex;
inline std::vector<Storage *> live_storages;

struct Storage {
  void *data{nullptr};
  std::size_t nbytes{0};
  Device device{Device::cpu};
  std::atomic<std::size_t> refcount{1};
  runtime::Allocator *allocator{nullptr};
  std::function<void(void *)> deleter{};
  std::string label;

  static Storage *create(std::size_t bytes, Device dev) {
    static runtime::CpuAllocator cpu_alloc;
    static runtime::MetalAllocator metal_alloc;

    runtime::Allocator *alloc = &cpu_alloc;
    if (dev == Device::mps) {
      alloc = &metal_alloc;
    }

    uuid_t id;
    uuid_generate(id);
    char uuid_str[37];
    uuid_unparse(id, uuid_str);

    void *ptr = alloc->allocate(bytes, uuid_str);
    if (!ptr) {
      throw std::runtime_error(
          "Storage allocation failed: missing Metal device");
    }

    Storage *st = new Storage;
    st->data = ptr;
    st->nbytes = bytes;
    st->device = dev;
    st->allocator = alloc;
    st->label = uuid_str;
    {
      std::lock_guard<std::mutex> g(live_storage_mutex);
      live_storages.push_back(st);
    }
    return st;
  }

  static Storage *wrap(void *data, std::size_t bytes, Device dev,
                       std::function<void(void *)> del = nullptr) {
    uuid_t id;
    uuid_generate(id);
    char uuid_str[37];
    uuid_unparse(id, uuid_str);
    Storage *st = new Storage;
    st->data = data;
    st->nbytes = bytes;
    st->device = dev;
    st->allocator = nullptr;
    st->deleter = std::move(del);
    st->label = uuid_str;
    {
      std::lock_guard<std::mutex> g(live_storage_mutex);
      live_storages.push_back(st);
    }
    std::ostringstream oss;
    oss << "alloc " << st->label << ' ' << bytes << ' ' << st->data;
    orchard::tensor_profile_log(oss.str());
    return st;
  }

  void retain() { refcount.fetch_add(1, std::memory_order_relaxed); }
  void release() {
    if (refcount.fetch_sub(1, std::memory_order_acq_rel) == 1) {
      if (allocator) {
        allocator->deallocate(data, label.c_str());
      } else {
        std::ostringstream oss;
        oss << "free " << label << ' ' << data;
        orchard::tensor_profile_log(oss.str());
        if (deleter)
          deleter(data);
      }
      {
        std::lock_guard<std::mutex> g(live_storage_mutex);
        auto it = std::find(live_storages.begin(), live_storages.end(), this);
        if (it != live_storages.end())
          live_storages.erase(it);
      }
      delete this;
    }
  }
};

} // namespace orchard::core::tensor
