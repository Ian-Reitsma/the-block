#pragma once

#include <cstddef>

namespace orchard::core::tensor {

enum class DType {
  f32,
  bf16,
  f16,
  u8,
  i32,
};

enum class Device {
  cpu,
  mps,
};

inline constexpr std::size_t dtype_size(DType dt) {
  switch (dt) {
    case DType::f32: return 4;
    case DType::bf16: return 2;
    case DType::f16: return 2;
    case DType::u8: return 1;
    case DType::i32: return 4;
  }
  return 0;
}

inline const char* device_name(Device d) {
  switch (d) {
    case Device::cpu: return "cpu";
    case Device::mps: return "mps";
  }
  return "unknown";
}

} // namespace orchard::core::tensor
