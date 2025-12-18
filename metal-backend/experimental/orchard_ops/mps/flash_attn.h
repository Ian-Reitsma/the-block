#ifndef ORCHARD_OPS_MPS_FLASH_ATTN_H
#define ORCHARD_OPS_MPS_FLASH_ATTN_H
#pragma once

// orchard_ops/mps/flash_attn.h
//
// Orchard Metal FlashAttention C++ interface header
// - Forward: Calls fused Metal kernel if available, else falls back to PyTorch
// reference.
// - Backward: Currently calls PyTorch reference unless/until Metal kernel is
// implemented.
//
// NOTE: All tensors must be allocated on the 'mps' device for correct
// execution.
//       Inputs on other devices will result in undefined behavior or runtime
//       error. (This file assumes PyTorch C++/ATen API conventions.)
//
// API subject to change as backward kernel matures and new tuning params may be
// added. Minimum required C++ standard: C++20.

// --- API versioning (increment if params/signature change) ---
#define ORCHARD_FLASH_ATTN_API_LEVEL 1

#include <ATen/ATen.h>

namespace orchard_ops { // Top-level namespace for all future kernels

// --- Tuning params struct (placeholder for future extension) ---
// If/when custom Metal tuning knobs are needed, add them here
// (Example: tile size, precision, dropout flags, etc.)
struct FlashAttnTuning {
  // Example: int tile_size = 0;   // 0 = default
  // Example: bool enable_dropout = false;
  // Example: float softmax_scale = 1.0f;
  // (Not yet used in API, but reserved for future expansion.)
};

// Forward pass for FlashAttention.
// Calls the Metal kernel (if available), else falls back to the PyTorch
// implementation. Precondition: q, k, v must all be 'mps' device tensors,
// shape- and dtype-compatible. tuning: optional pointer to tuning struct
// (future use; pass nullptr for default behavior) Returns attention output and
// dropout mask.
std::tuple<at::Tensor, at::Tensor>
orchard_flash_attn_fwd(const at::Tensor &q, const at::Tensor &k,
                       const at::Tensor &v,
                       double scale, // <-- DOUBLE
                       double dropout_p, bool causal
                       //, const FlashAttnTuning* tuning = nullptr  // Uncomment
                       // when tuning supported
);

// Backward pass for FlashAttention (stub).
// Returns gradients w.r.t. q, k, v in a tuple.
// Current implementation falls back to PyTorch's
// scaled_dot_product_attention_backward. API subject to change as the native
// Metal kernel is developed. tuning: optional pointer to tuning struct (future
// use; pass nullptr for default behavior)
std::tuple<at::Tensor, at::Tensor, at::Tensor>
orchard_flash_attn_bwd(const at::Tensor &grad, const at::Tensor &q,
                       const at::Tensor &k, const at::Tensor &v,
                       const at::Tensor &dropout_mask,
                       double scale, // <-- DOUBLE
                       double dropout_p, bool causal
                       //, const FlashAttnTuning* tuning = nullptr  // Uncomment
                       // when tuning supported
);

} // namespace orchard_ops

#endif // ORCHARD_OPS_MPS_FLASH_ATTN_H
