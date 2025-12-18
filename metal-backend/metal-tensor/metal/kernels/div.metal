#include <metal_stdlib>
using namespace metal;

kernel void div_arrays(const device float *a [[buffer(0)]],
                       const device float *b [[buffer(1)]],
                       device float *c [[buffer(2)]],
                       const device long *shape [[buffer(3)]],
                       const device long *astrides [[buffer(4)]],
                       const device long *bstrides [[buffer(5)]],
                       constant int &safe [[buffer(6)]],
                       uint gid [[thread_position_in_grid]]) {
  long ao = 0;
  long bo = 0;
  uint idx = gid;
  for (uint d = 0; d < 8; ++d) {
    long s = shape[d];
    long i = idx % s;
    idx /= s;
    ao += i * astrides[d];
    bo += i * bstrides[d];
  }
  float bv = b[bo];
  c[gid] = (safe && bv == 0.0f) ? 0.0f : a[ao] / bv;
}

kernel void div_scalar(const device float *a [[buffer(0)]],
                       constant float &s [[buffer(1)]],
                       device float *c [[buffer(2)]],
                       constant int &safe [[buffer(3)]],
                       uint gid [[thread_position_in_grid]]) {
  c[gid] = (safe && s == 0.0f) ? 0.0f : a[gid] / s;
}

kernel void div_backward_a(const device float *grad [[buffer(0)]],
                           const device float *b [[buffer(1)]],
                           device float *out [[buffer(2)]],
                           uint gid [[thread_position_in_grid]]) {
  out[gid] = grad[gid] / b[gid];
}

kernel void div_backward_b(const device float *grad [[buffer(0)]],
                           const device float *a [[buffer(1)]],
                           const device float *b [[buffer(2)]],
                           device float *out [[buffer(3)]],
                           uint gid [[thread_position_in_grid]]) {
  out[gid] = -grad[gid] * a[gid] / (b[gid] * b[gid]);
}
