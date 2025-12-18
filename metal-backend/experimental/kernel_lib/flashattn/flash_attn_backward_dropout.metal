// experimental/kernel_lib/flashattn/flash_attn_backward_dropout.metal
//
// Fused backward FlashAttention kernel with dropout support. Applies the
// provided dropout mask and scaling factor to the incoming gradient and
// produces gradients for query, key and value tensors. This minimal Metal
// implementation mirrors the Objective\-C++ launcher in
// `orchard_ops/mps/flash_attn.mm` and is suitable for precompilation into the
// FlashAttention kernel library.

#include <metal_stdlib>
using namespace metal;

kernel void flash_attn_bwd_dropout(
    const device float *grad_out [[buffer(0)]],
    const device float *q [[buffer(1)]],
    const device float *k [[buffer(2)]],
    const device float *v [[buffer(3)]],
    const device float *mask [[buffer(4)]],
    device float *grad_q [[buffer(5)]],
    device float *grad_k [[buffer(6)]],
    device float *grad_v [[buffer(7)]],
    constant uint &n [[buffer(8)]],
    constant float &scale [[buffer(9)]],
    constant float &dropout_p [[buffer(10)]],
    constant bool &causal [[buffer(11)]],
    uint gid [[thread_position_in_grid]]) {
  if (gid >= n) {
    return;
  }
  // Apply dropout mask and rescale to maintain expected value.
  float g = grad_out[gid] * mask[gid] / (1.0 - dropout_p);
  grad_q[gid] = g * scale;
  grad_k[gid] = g * scale;
  grad_v[gid] = g;
}
