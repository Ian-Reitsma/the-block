#include <metal_stdlib>
using namespace metal;

kernel void reduce_sum(const device float *a [[buffer(0)]],
                       device float *out [[buffer(1)]],
                       constant uint &n [[buffer(2)]],
                       uint gid [[thread_position_in_grid]]) {
  if (gid == 0) {
    float s = 0.0;
    for (uint i = 0; i < n; ++i)
      s += a[i];
    out[0] = s;
  }
}
