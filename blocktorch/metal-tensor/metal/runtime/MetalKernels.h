#pragma once

#include <cstddef>
#include <cstdint>

namespace orchard::runtime {
void metal_add(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims, std::size_t n);
void metal_mul(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims, std::size_t n);
void metal_div(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims, std::size_t n,
               bool safe);
void metal_div_scalar(const float *a, float scalar, float *out, std::size_t n,
                      bool safe);
void metal_matmul(const float *a, const float *b, float *c, std::size_t m,
                  std::size_t n, std::size_t k);
void metal_reduce_sum(const float *a, float *out, std::size_t n);
void metal_mean(const float *a, float *out, std::size_t n);
void metal_reduce_sum_axis(const float *a, float *out,
                           const std::int64_t *shape,
                           const std::int64_t *strides, std::uint32_t dims,
                           std::uint32_t axis_len, std::uint32_t axis,
                           std::size_t n);
void metal_mean_axis(const float *a, float *out, const std::int64_t *shape,
                     const std::int64_t *strides, std::uint32_t dims,
                     std::uint32_t axis_len, std::uint32_t axis, std::size_t n);
void metal_mul_backward_a(const float *g, const float *b, float *ga,
                          std::size_t n);
void metal_mul_backward_b(const float *g, const float *a, float *gb,
                          std::size_t n);
void metal_div_backward_a(const float *g, const float *b, float *ga,
                          std::size_t n);
void metal_div_backward_b(const float *g, const float *a, const float *b,
                          float *gb, std::size_t n);
void metal_transpose_backward(const float *g, float *out, std::size_t m,
                              std::size_t n);
// Backward matmul kernels expect dimensions in (m, n, k) order
void metal_matmul_backward_a(const float *g, const float *b, float *ga,
                             std::size_t m, std::size_t n, std::size_t k);
void metal_matmul_backward_b(const float *g, const float *a, float *gb,
                             std::size_t m, std::size_t n, std::size_t k);
void metal_fill(float *out, float value, std::size_t n);
} // namespace orchard::runtime
