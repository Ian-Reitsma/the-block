#include <metal_stdlib>
using namespace metal;

kernel void transpose_backward(const device float *grad [[buffer(0)]],
                               device float *out [[buffer(1)]],
                               constant uint &m [[buffer(2)]],
                               constant uint &n [[buffer(3)]],
                               uint gid [[thread_position_in_grid]]) {
  uint row = gid / n;
  uint col = gid % n;
  if (row >= m || col >= n)
    return;
  out[row * n + col] = grad[col * m + row];
}
