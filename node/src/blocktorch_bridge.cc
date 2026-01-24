#include <cstddef>

#include "../../blocktorch/metal-tensor/metal/runtime/CpuContext.h"

extern "C" bool blocktorch_cpu_add(const float *left, const float *right,
                                   std::size_t len, float *out) {
  if (!left || !right || !out) {
    return false;
  }
  orchard::runtime::cpu_context().add(left, right, out, len);
  return true;
}
