#include <metal_stdlib>
using namespace metal;

kernel void matmul_kernel(const device float *a [[buffer(0)]],
                          const device float *b [[buffer(1)]],
                          device float *c [[buffer(2)]],
                          constant uint &m [[buffer(3)]],
                          constant uint &n [[buffer(4)]],
                          constant uint &k [[buffer(5)]],
                          uint gid [[thread_position_in_grid]]) {
  uint row = gid / n;
  uint col = gid % n;
  if (row >= m || col >= n)
    return;
  float acc = 0.0;
  for (uint p = 0; p < k; ++p)
    acc += a[row * k + p] * b[p * n + col];
  c[row * n + col] = acc;
}
