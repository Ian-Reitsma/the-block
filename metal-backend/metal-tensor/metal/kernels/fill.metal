#include <metal_stdlib>
using namespace metal;

kernel void fill_value(device float *out [[buffer(0)]],
                       constant float &v [[buffer(1)]],
                       constant uint &n [[buffer(2)]],
                       uint gid [[thread_position_in_grid]]) {
  if (gid >= n)
    return;
  out[gid] = v;
}
