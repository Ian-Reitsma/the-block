#pragma once

#include <cstddef>

#ifdef __APPLE__
#include <Accelerate/Accelerate.h>
#endif

namespace orchard::runtime {

/// Lightweight wrapper around Accelerate/BNNS utilities for CPU‑side ops.
class CpuContext {
public:
  CpuContext() = default;

  /// Elementwise vector addition using Accelerate when available.
  void add(const float *a, const float *b, float *c, std::size_t n) const;
};

/// Obtain the CPU context associated with the calling thread.
CpuContext &cpu_context();

} // namespace orchard::runtime

// Inline implementation
inline void orchard::runtime::CpuContext::add(const float *a, const float *b,
                                              float *c, std::size_t n) const {
#ifdef __APPLE__
  vDSP_vadd(a, 1, b, 1, c, 1, n);
#else
  for (std::size_t i = 0; i < n; ++i) {
    c[i] = a[i] + b[i];
  }
#endif
}

inline orchard::runtime::CpuContext &orchard::runtime::cpu_context() {
  thread_local CpuContext ctx;
  return ctx;
}

