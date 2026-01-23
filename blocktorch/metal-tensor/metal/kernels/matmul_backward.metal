#include <metal_stdlib>
using namespace metal;

kernel void matmul_backward_a(const device float *grad [[buffer(0)]],
                              const device float *b [[buffer(1)]],
                              device float *out [[buffer(2)]],
                              constant uint &m [[buffer(3)]],
                              constant uint &n [[buffer(4)]],
                              constant uint &k [[buffer(5)]],
                              uint gid [[thread_position_in_grid]]) {
  uint row = gid / k;
  uint col = gid % k;
  if (row >= m || col >= k)
    return;
  float s = 0.0f;
  for (uint p = 0; p < n; ++p)
    s += grad[row * n + p] * b[col * n + p];
  out[row * k + col] = s;
}

kernel void matmul_backward_b(const device float *grad [[buffer(0)]],
                              const device float *a [[buffer(1)]],
                              device float *out [[buffer(2)]],
                              constant uint &m [[buffer(3)]],
                              constant uint &n [[buffer(4)]],
                              constant uint &k [[buffer(5)]],
                              uint gid [[thread_position_in_grid]]) {
  uint row = gid / n;
  uint col = gid % n;
  if (row >= k || col >= n)
    return;
  float s = 0.0f;
  for (uint p = 0; p < m; ++p)
    s += a[p * k + row] * grad[p * n + col];
  out[row * n + col] = s;
}
