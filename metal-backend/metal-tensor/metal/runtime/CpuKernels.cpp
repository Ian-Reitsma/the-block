#include "MetalKernels.h"

#include <array>
#include <cstring>
#include <vector>

namespace orchard::runtime {

void metal_add(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims,
               std::size_t n) {
  std::vector<std::int64_t> shp(shape, shape + dims);
  std::vector<std::int64_t> as(astrides, astrides + dims);
  std::vector<std::int64_t> bs(bstrides, bstrides + dims);
  std::vector<std::int64_t> idx(dims);
  std::int64_t ao = 0;
  std::int64_t bo = 0;
  for (std::size_t i = 0; i < n; ++i) {
    c[i] = a[ao] + b[bo];
    for (int d = dims - 1; d >= 0; --d) {
      idx[d]++;
      ao += as[d];
      if (bs[d] != 0)
        bo += bs[d];
      if (idx[d] < shp[d])
        break;
      idx[d] = 0;
      ao -= as[d] * shp[d];
      if (bs[d] != 0)
        bo -= bs[d] * shp[d];
    }
  }
}

void metal_div_scalar(const float *a, float scalar, float *out, std::size_t n,
                      bool safe) {
  if (safe && scalar == 0.0f) {
    for (std::size_t i = 0; i < n; ++i)
      out[i] = 0.0f;
  } else {
    for (std::size_t i = 0; i < n; ++i)
      out[i] = a[i] / scalar;
  }
}

void metal_mul(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims,
               std::size_t n) {
  std::vector<std::int64_t> shp(shape, shape + dims);
  std::vector<std::int64_t> as(astrides, astrides + dims);
  std::vector<std::int64_t> bs(bstrides, bstrides + dims);
  std::vector<std::int64_t> idx(dims);
  std::int64_t ao = 0;
  std::int64_t bo = 0;
  for (std::size_t i = 0; i < n; ++i) {
    c[i] = a[ao] * b[bo];
    for (int d = dims - 1; d >= 0; --d) {
      idx[d]++;
      ao += as[d];
      if (bs[d] != 0)
        bo += bs[d];
      if (idx[d] < shp[d])
        break;
      idx[d] = 0;
      ao -= as[d] * shp[d];
      if (bs[d] != 0)
        bo -= bs[d] * shp[d];
    }
  }
}

void metal_div(const float *a, const float *b, float *c,
               const std::int64_t *shape, const std::int64_t *astrides,
               const std::int64_t *bstrides, std::uint32_t dims, std::size_t n,
               bool safe) {
  std::vector<std::int64_t> shp(shape, shape + dims);
  std::vector<std::int64_t> as(astrides, astrides + dims);
  std::vector<std::int64_t> bs(bstrides, bstrides + dims);
  std::vector<std::int64_t> idx(dims);
  std::int64_t ao = 0;
  std::int64_t bo = 0;
  for (std::size_t i = 0; i < n; ++i) {
    float bv = b[bo];
    c[i] = (safe && bv == 0.0f) ? 0.0f : a[ao] / bv;
    for (int d = dims - 1; d >= 0; --d) {
      idx[d]++;
      ao += as[d];
      if (bs[d] != 0)
        bo += bs[d];
      if (idx[d] < shp[d])
        break;
      idx[d] = 0;
      ao -= as[d] * shp[d];
      if (bs[d] != 0)
        bo -= bs[d] * shp[d];
    }
  }
}

void metal_mul_backward_a(const float *g, const float *b, float *ga,
                          std::size_t n) {
  for (std::size_t i = 0; i < n; ++i)
    ga[i] = g[i] * b[i];
}

void metal_mul_backward_b(const float *g, const float *a, float *gb,
                          std::size_t n) {
  for (std::size_t i = 0; i < n; ++i)
    gb[i] = g[i] * a[i];
}

void metal_div_backward_a(const float *g, const float *b, float *ga,
                          std::size_t n) {
  for (std::size_t i = 0; i < n; ++i)
    ga[i] = g[i] / b[i];
}

void metal_div_backward_b(const float *g, const float *a, const float *b,
                          float *gb, std::size_t n) {
  for (std::size_t i = 0; i < n; ++i)
    gb[i] = -g[i] * a[i] / (b[i] * b[i]);
}

void metal_matmul(const float *a, const float *b, float *c, std::size_t m,
                  std::size_t n, std::size_t k) {
  for (std::size_t i = 0; i < m; ++i) {
    for (std::size_t j = 0; j < n; ++j) {
      float s = 0.0f;
      for (std::size_t p = 0; p < k; ++p)
        s += a[i * k + p] * b[p * n + j];
      c[i * n + j] = s;
    }
  }
}

void metal_reduce_sum(const float *a, float *out, std::size_t n) {
  float s = 0.0f;
  for (std::size_t i = 0; i < n; ++i)
    s += a[i];
  out[0] = s;
}

void metal_mean(const float *a, float *out, std::size_t n) {
  float s = 0.0f;
  for (std::size_t i = 0; i < n; ++i)
    s += a[i];
  out[0] = s / static_cast<float>(n);
}

void metal_reduce_sum_axis(const float *a, float *out,
                           const std::int64_t *shape,
                           const std::int64_t *strides, std::uint32_t dims,
                           std::uint32_t axis_len, std::uint32_t axis,
                           std::size_t n) {
  std::vector<std::int64_t> shp(shape, shape + dims);
  std::vector<std::int64_t> st(strides, strides + dims);
  for (std::size_t i = 0; i < n; ++i) {
    std::size_t idx = i;
    long base = 0;
    for (int d = dims - 1; d >= 0; --d) {
      long s = shp[d];
      long coord = idx % s;
      idx /= s;
      base += coord * st[d];
    }
    float s = 0.0f;
    long pos = base;
    for (std::uint32_t j = 0; j < axis_len; ++j) {
      s += a[pos];
      pos += st[axis];
    }
    out[i] = s;
  }
}

void metal_mean_axis(const float *a, float *out, const std::int64_t *shape,
                     const std::int64_t *strides, std::uint32_t dims,
                     std::uint32_t axis_len, std::uint32_t axis,
                     std::size_t n) {
  std::vector<std::int64_t> shp(shape, shape + dims);
  std::vector<std::int64_t> st(strides, strides + dims);
  for (std::size_t i = 0; i < n; ++i) {
    std::size_t idx = i;
    long base = 0;
    for (int d = dims - 1; d >= 0; --d) {
      long s = shp[d];
      long coord = idx % s;
      idx /= s;
      base += coord * st[d];
    }
    float s = 0.0f;
    long pos = base;
    for (std::uint32_t j = 0; j < axis_len; ++j) {
      s += a[pos];
      pos += st[axis];
    }
    out[i] = s / static_cast<float>(axis_len);
  }
}

// Parameters follow (m, n, k)
void metal_matmul_backward_a(const float *g, const float *b, float *ga,
                             std::size_t m, std::size_t n, std::size_t k) {
  for (std::size_t i = 0; i < m; ++i) {
    for (std::size_t j = 0; j < k; ++j) {
      float s = 0.0f;
      for (std::size_t p = 0; p < n; ++p)
        s += g[i * n + p] * b[j * n + p];
      ga[i * k + j] = s;
    }
  }
}

// Parameters follow (m, n, k)
void metal_matmul_backward_b(const float *g, const float *a, float *gb,
                             std::size_t m, std::size_t n, std::size_t k) {
  for (std::size_t i = 0; i < k; ++i) {
    for (std::size_t j = 0; j < n; ++j) {
      float s = 0.0f;
      for (std::size_t p = 0; p < m; ++p)
        s += a[p * k + i] * g[p * n + j];
      gb[i * n + j] = s;
    }
  }
}

void metal_transpose_backward(const float *g, float *out, std::size_t m,
                              std::size_t n) {
  for (std::size_t i = 0; i < m; ++i) {
    for (std::size_t j = 0; j < n; ++j) {
      out[i * n + j] = g[j * m + i];
    }
  }
}

void metal_fill(float *out, float value, std::size_t n) {
  for (std::size_t i = 0; i < n; ++i)
    out[i] = value;
}
} // namespace orchard::runtime
