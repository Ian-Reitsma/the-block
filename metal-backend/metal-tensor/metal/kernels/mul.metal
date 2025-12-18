#include <metal_stdlib>
using namespace metal;

kernel void mul_arrays(const device float *a [[buffer(0)]],
                       const device float *b [[buffer(1)]],
                       device float *c [[buffer(2)]],
                       const device long *shape [[buffer(3)]],
                       const device long *astrides [[buffer(4)]],
                       const device long *bstrides [[buffer(5)]],
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
  c[gid] = a[ao] * b[bo];
}
