#include <gtest/gtest.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <fstream>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

#include "common/Profiling.h"
#include "core/tensor/Debug.h"
#include "core/tensor/Tensor.h"
#include "runtime/MetalContext.h"

using namespace orchard::core::tensor;

TEST(MultiDeviceTransferTest, CpuMetalCpuTwoTensors) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  Tensor b = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  auto *bp = static_cast<float *>(b.data_ptr());
  for (int i = 0; i < 4; ++i) {
    ap[i] = static_cast<float>(i);
    bp[i] = static_cast<float>(i + 10);
  }
#ifdef __APPLE__
  Tensor ma = a.to(Device::mps);
  Tensor mb = b.to(Device::mps);
  Tensor ra = ma.to(Device::cpu);
  Tensor rb = mb.to(Device::cpu);
  auto *rap = static_cast<float *>(ra.data_ptr());
  auto *rbp = static_cast<float *>(rb.data_ptr());
  for (int i = 0; i < 4; ++i) {
    EXPECT_FLOAT_EQ(rap[i], ap[i]);
    EXPECT_FLOAT_EQ(rbp[i], bp[i]);
  }
#endif
}

TEST(MultiDeviceTransferTest, MixedCpuMetalSequence) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ap = static_cast<float *>(a.data_ptr());
  for (int i = 0; i < 4; ++i)
    ap[i] = static_cast<float>(i * 3);
#ifdef __APPLE__
  Tensor m1 = a.to(Device::mps);
  Tensor c1 = m1.to(Device::cpu);
  Tensor m2 = c1.to(Device::mps);
  Tensor c2 = m2.to(Device::cpu);
  auto *cp = static_cast<float *>(c2.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(cp[i], ap[i]);
#endif
}

TEST(MultiDeviceTransferTest, NonContiguousZeroCopyAndAlignment) {
  std::array<std::int64_t, 8> shape{8, 1, 1, 1, 1, 1, 1, 1};
  Tensor base = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *ptr = static_cast<float *>(base.data_ptr());
  for (int i = 0; i < 8; ++i)
    ptr[i] = static_cast<float>(i);
  Tensor slice = base.slice(0, 1, 8, 2);
  EXPECT_FALSE(slice.is_contiguous());
  Tensor view = slice.to(Device::cpu);
  EXPECT_EQ(view.data_ptr(), slice.data_ptr());
  std::uintptr_t addr = reinterpret_cast<std::uintptr_t>(view.data_ptr());
  EXPECT_NE(addr % 64, 0u);
#ifdef __APPLE__
  Tensor metal = slice.to(Device::mps);
  std::uintptr_t maddr = reinterpret_cast<std::uintptr_t>(metal.data_ptr());
  EXPECT_EQ(maddr % 64, 0u);
  Tensor back = metal.to(Device::cpu);
  auto *bptr = static_cast<float *>(back.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(bptr[i], static_cast<float>(i * 2 + 1));
#endif
}

TEST(ProfilingStressTest, AllocationAndQueuePooling) {
  orchard::tensor_profile_clear_log();
  unsetenv("ORCHARD_TENSOR_PROFILE");
  orchard::tensor_profile_reset();
  setenv("ORCHARD_TENSOR_PROFILE", "1", 1);
  orchard::tensor_profile_reset();
  std::array<std::int64_t, 8> shape{1024 * 1024, 1, 1, 1, 1, 1, 1, 1};
  const int threads = 4;
  std::atomic<bool> ok{true};
  auto worker = [&]() {
    for (int i = 0; i < 10; ++i) {
      Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
      auto *cp = static_cast<float *>(cpu.data_ptr());
      for (int j = 0; j < 1024 * 1024; ++j)
        cp[j] = static_cast<float>(j);
#ifdef __APPLE__
      Tensor metal = cpu.to(Device::mps);
      Tensor back = metal.to(Device::cpu);
      auto *bp = static_cast<float *>(back.data_ptr());
      for (int j = 0; j < 1024 * 1024; ++j) {
        if (bp[j] != cp[j]) {
          ok = false;
          break;
        }
      }
#endif
    }
  };
  std::vector<std::thread> workers;
  for (int t = 0; t < threads; ++t)
    workers.emplace_back(worker);
  for (auto &w : workers)
    w.join();
  dump_live_tensors();
  unsetenv("ORCHARD_TENSOR_PROFILE");
  orchard::tensor_profile_reset();
  std::ifstream ifs("/tmp/orchard_tensor_profile.log");
  EXPECT_TRUE(ifs.good());
  std::size_t allocs = 0, frees = 0;
  std::unordered_map<std::string, int> balance;
  for (std::string line; std::getline(ifs, line);) {
    std::istringstream iss(line);
    std::string tag, label;
    iss >> tag >> label;
    if (tag == "alloc") {
      ++allocs;
      ++balance[label];
    } else if (tag == "free") {
      ++frees;
      --balance[label];
    }
  }
  EXPECT_EQ(allocs, frees);
  for (auto &p : balance)
    EXPECT_EQ(p.second, 0);
  EXPECT_GT(allocs, 0u);
  EXPECT_GT(frees, 0u);
  EXPECT_TRUE(ok);
}

TEST(ProfilingStressTest, NoLoggingWhenUnset) {
  orchard::tensor_profile_clear_log();
  unsetenv("ORCHARD_TENSOR_PROFILE");
  orchard::tensor_profile_reset();
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
#ifdef __APPLE__
  Tensor metal = cpu.to(Device::mps);
  (void)metal.to(Device::cpu);
#endif
  dump_live_tensors();
  std::ifstream ifs("/tmp/orchard_tensor_profile.log");
  EXPECT_FALSE(ifs.good());
}

#ifdef __APPLE__
TEST(MultiDeviceTransferTest, MetalToMetalCopy) {
  std::array<std::int64_t, 8> shape{4, 1, 1, 1, 1, 1, 1, 1};
  Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
  auto *cp = static_cast<float *>(cpu.data_ptr());
  for (int i = 0; i < 4; ++i)
    cp[i] = static_cast<float>(i + 1);
  Tensor m1 = cpu.to(Device::mps);
  Tensor m2 = m1.to(Device::mps);
  Tensor back = m2.to(Device::cpu);
  auto *bp = static_cast<float *>(back.data_ptr());
  for (int i = 0; i < 4; ++i)
    EXPECT_FLOAT_EQ(bp[i], cp[i]);
}
#endif

#ifdef __APPLE__
TEST(MultiDeviceTransferTest, MultiThreadedLargeTransfers) {
  std::array<std::int64_t, 8> shape{1024 * 1024, 1, 1, 1, 1, 1, 1, 1};
  std::atomic<bool> ok{true};
  auto thr = [&]() {
    Tensor cpu = Tensor::empty(shape, DType::f32, Device::cpu);
    auto *cp = static_cast<float *>(cpu.data_ptr());
    for (int i = 0; i < 1024 * 1024; ++i)
      cp[i] = static_cast<float>(i);
    Tensor m = cpu.to(Device::mps);
    Tensor back = m.to(Device::cpu);
    auto *bp = static_cast<float *>(back.data_ptr());
    for (int i = 0; i < 1024 * 1024; ++i) {
      if (bp[i] != cp[i]) {
        ok = false;
        break;
      }
    }
  };
  std::vector<std::thread> threads;
  for (int i = 0; i < 4; ++i)
    threads.emplace_back(thr);
  for (auto &t : threads)
    t.join();
  EXPECT_TRUE(ok);
}
#endif
