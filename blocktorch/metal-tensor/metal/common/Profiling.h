#pragma once

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <mutex>
#include <string>

namespace orchard {

inline std::atomic<int> &tensor_profile_state() {
  static std::atomic<int> state{-1};
  return state;
}

inline bool tensor_profile_enabled() {
  int s = tensor_profile_state().load(std::memory_order_acquire);
  if (s == -1) {
    s = std::getenv("ORCHARD_TENSOR_PROFILE") ? 1 : 0;
    tensor_profile_state().store(s, std::memory_order_release);
  }
  return s == 1;
}

inline void tensor_profile_reset() {
  tensor_profile_state().store(-1, std::memory_order_release);
}

inline void tensor_profile_clear_log() {
  std::remove("/tmp/orchard_tensor_profile.log");
}

inline void tensor_profile_log(const std::string &msg) {
  if (!tensor_profile_enabled())
    return;
  static std::mutex m;
  std::lock_guard<std::mutex> lock(m);
  std::ofstream ofs("/tmp/orchard_tensor_profile.log", std::ios::app);
  ofs << msg << '\n';
}

} // namespace orchard
