#include <metal_stdlib>
using namespace metal;

kernel void mean_axis(const device float *a [[buffer(0)]],
                      device float *out [[buffer(1)]],
                      const device long *shape [[buffer(2)]],
                      const device long *strides [[buffer(3)]],
                      constant uint &axis_len [[buffer(4)]],
                      constant uint &axis [[buffer(5)]],
                      uint gid [[thread_position_in_grid]]) {
  long base = 0;
  uint idx = gid;
  for (uint d = 0; d < 8; ++d) {
    long s = shape[d];
    long i = idx % s;
    idx /= s;
    base += i * strides[d];
  }
  long step = strides[axis];
  float s = 0.0;
  long pos = base;
  for (uint i = 0; i < axis_len; ++i) {
    s += a[pos];
    pos += step;
  }
  out[gid] = s / static_cast<float>(axis_len);
}
