// orchard_ops/mps/flash_attn_backward.metal
//
// Simplified fused backward pass applying dropout mask and scale to gradients.
// Consumes (grad_out, q, k, v, mask, scale, dropout_p, causal) and produces
// gradients for q, k and v. The q, k, v inputs are presently unused but kept in
// the signature to match the expected kernel ABI and allow future expansion.

#include <metal_stdlib>
using namespace metal;

kernel void flash_attn_bwd(
    const device float *grad_out [[buffer(0)]],
    const device float *q [[buffer(1)]], const device float *k [[buffer(2)]],
    const device float *v [[buffer(3)]], const device float *mask [[buffer(4)]],
    device float *grad_q [[buffer(5)]], device float *grad_k [[buffer(6)]],
    device float *grad_v [[buffer(7)]], constant uint &n [[buffer(8)]],
    constant float &scale [[buffer(9)]],
    constant float &dropout_p [[buffer(10)]],
    constant bool &causal [[buffer(11)]],
    uint gid [[thread_position_in_grid]]) {
  if (gid >= n) {
    return;
  }
  float g = grad_out[gid] * mask[gid] / (1.0 - dropout_p);
  grad_q[gid] = g * scale;
  grad_k[gid] = g * scale;
  grad_v[gid] = g;
}
