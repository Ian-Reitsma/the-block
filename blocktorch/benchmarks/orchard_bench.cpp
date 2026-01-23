#include "core/tensor/Tensor.h"
#include <array>
#include <chrono>
#include <cstdint>
#include <iostream>
#include <string>

using namespace orchard::core::tensor;

namespace {

double bench_add(std::int64_t elements) {
  std::array<std::int64_t, 8> shape{elements, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::mps);
  Tensor b = Tensor::empty(shape, DType::f32, Device::mps);
  auto start = std::chrono::high_resolution_clock::now();
  Tensor c = a.add(b);
  c = c.to(Device::cpu);
  auto end = std::chrono::high_resolution_clock::now();
  return std::chrono::duration<double>(end - start).count();
}

double bench_mul(std::int64_t elements) {
  std::array<std::int64_t, 8> shape{elements, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::mps);
  Tensor b = Tensor::empty(shape, DType::f32, Device::mps);
  auto start = std::chrono::high_resolution_clock::now();
  Tensor c = a.mul(b);
  c = c.to(Device::cpu);
  auto end = std::chrono::high_resolution_clock::now();
  return std::chrono::duration<double>(end - start).count();
}

double bench_matmul(std::int64_t m, std::int64_t n, std::int64_t k) {
  std::array<std::int64_t, 8> aShape{m, k, 1, 1, 1, 1, 1, 1};
  std::array<std::int64_t, 8> bShape{k, n, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(aShape, DType::f32, Device::mps);
  Tensor b = Tensor::empty(bShape, DType::f32, Device::mps);
  auto start = std::chrono::high_resolution_clock::now();
  Tensor c = a.matmul(b);
  c = c.to(Device::cpu);
  auto end = std::chrono::high_resolution_clock::now();
  return std::chrono::duration<double>(end - start).count();
}

double bench_reduce_sum(std::int64_t elements) {
  std::array<std::int64_t, 8> shape{elements, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::mps);
  auto start = std::chrono::high_resolution_clock::now();
  Tensor s = a.sum();
  s = s.to(Device::cpu);
  auto end = std::chrono::high_resolution_clock::now();
  return std::chrono::duration<double>(end - start).count();
}

double bench_mean(std::int64_t elements) {
  std::array<std::int64_t, 8> shape{elements, 1, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::mps);
  auto start = std::chrono::high_resolution_clock::now();
  Tensor m = a.mean();
  m = m.to(Device::cpu);
  auto end = std::chrono::high_resolution_clock::now();
  return std::chrono::duration<double>(end - start).count();
}

double bench_transpose(std::int64_t m, std::int64_t n) {
  std::array<std::int64_t, 8> shape{m, n, 1, 1, 1, 1, 1, 1};
  Tensor a = Tensor::empty(shape, DType::f32, Device::mps);
  auto start = std::chrono::high_resolution_clock::now();
  Tensor t = a.transpose(0, 1).contiguous();
  t = t.to(Device::cpu);
  auto end = std::chrono::high_resolution_clock::now();
  return std::chrono::duration<double>(end - start).count();
}

} // namespace

int main(int argc, char **argv) {
  if (argc < 2) {
    std::cerr << "usage: orchard_bench "
                 "<add|mul|matmul|reduce_sum|mean|transpose> [sizes]\n";
    return 1;
  }
  std::string op = argv[1];
  if (op == "add") {
    std::int64_t n = argc > 2 ? std::stoll(argv[2]) : 1000000;
    std::cout << bench_add(n) << "\n";
    return 0;
  }
  if (op == "mul") {
    std::int64_t n = argc > 2 ? std::stoll(argv[2]) : 1000000;
    std::cout << bench_mul(n) << "\n";
    return 0;
  }
  if (op == "matmul") {
    std::int64_t m = argc > 2 ? std::stoll(argv[2]) : 64;
    std::int64_t n = argc > 3 ? std::stoll(argv[3]) : 64;
    std::int64_t k = argc > 4 ? std::stoll(argv[4]) : 64;
    std::cout << bench_matmul(m, n, k) << "\n";
    return 0;
  }
  if (op == "reduce_sum") {
    std::int64_t n = argc > 2 ? std::stoll(argv[2]) : 1000000;
    std::cout << bench_reduce_sum(n) << "\n";
    return 0;
  }
  if (op == "mean") {
    std::int64_t n = argc > 2 ? std::stoll(argv[2]) : 1000000;
    std::cout << bench_mean(n) << "\n";
    return 0;
  }
  if (op == "transpose") {
    std::int64_t m = argc > 2 ? std::stoll(argv[2]) : 1024;
    std::int64_t n = argc > 3 ? std::stoll(argv[3]) : 1024;
    std::cout << bench_transpose(m, n) << "\n";
    return 0;
  }
  std::cerr << "unknown kernel" << std::endl;
  return 1;
}
