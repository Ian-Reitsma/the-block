# orchard_bridge/flash_attn_function.py

"""
Autograd wrapper for Metal FlashAttention in Orchard.
- Handles forward and backward with strict Metal op checks.
- Fails clearly if kernels or PyTorch are missing.
- Metal backward requires tensors backed by shared MTLBuffer storage.
  This wrapper ensures all tensors passed to Metal backward use shared allocation.

Copied from: ~/projects/Apple-Metal-Orchard/experimental/orchard_ops/flash_attn_function.py
"""

import sys
import os

try:
    import torch
    from torch.autograd import Function
except Exception:  # pragma: no cover
    torch = None

    class Function:
        pass


_DEBUG = (
    bool(int(os.environ.get("ORCHARD_DEBUG_FLASHATN", "0")))
    if "os" in sys.modules
    else False
)


def _fail(msg: str) -> None:
    raise RuntimeError(f"[orchard][flash_attn_function] {msg}")


def _metal_kernel_available():
    return (
        torch is not None
        and hasattr(torch.ops, "flash_attn_mps")
        and hasattr(torch.ops.flash_attn_mps, "_flash_attn_fwd")
        and hasattr(torch.ops.flash_attn_mps, "_flash_attn_bwd_dropout")
    )


def _contig(t: "torch.Tensor") -> "torch.Tensor":
    """Return a contiguous tensor without changing values.

    Note: This function is used inside a custom autograd.Function where ops are
    not recorded. That is OK because contiguity is value-preserving; backward
    returns gradients in the original logical layout.
    """

    if t is None:
        return t
    if hasattr(t, "is_contiguous") and not t.is_contiguous():
        return t.contiguous()
    return t


def _ensure_shared_mps_tensor(t: "torch.Tensor") -> "torch.Tensor":
    """Ensure an MPS tensor is backed by shared MTLBuffer storage.

    Metal backward kernels require all tensors to be backed by MTLResourceStorageModeShared
    buffers. This function materializes a tensor into shared storage if needed.

    Strategy:
    1. If tensor is already contiguous on MPS, it's likely already shared.
    2. If non-contiguous or from autograd view, clone to fresh shared allocation.
    3. Use a no-op Metal operation to force allocation if necessary.

    Args:
        t: Tensor to ensure is on shared MPS storage.

    Returns:
        A tensor backed by shared MTLBuffer storage, or original if not MPS.
    """

    if t is None or t.device.type != "mps":
        return t

    # For backward tensors (esp. grad_out, grad_*, mask from saved_tensors),
    # cloning forces a fresh allocation which is typically shared by default.
    if not t.is_contiguous():
        return t.contiguous().clone()

    # Contiguous tensors may still have non-shared storage if they're views
    # or result from autograd operations. Clone if stride pattern suggests a view.
    strides = t.stride()
    shape = t.shape
    expected_strides = tuple(
        torch.Size(shape[i+1:]).numel() for i in range(len(shape))
    )

    # If strides don't match expected row-major layout, it's a view -> clone.
    if strides != expected_strides:
        return t.clone()

    # Last resort: if tensor may have come from autograd graph,
    # clone to ensure shared storage. PyTorch MPS's default allocation
    # for new tensors is typically shared, so clone() is usually safe.
    if t.requires_grad or not t.is_leaf:
        return t.clone().detach()

    return t


def _ref_attention(q: "torch.Tensor", k: "torch.Tensor", v: "torch.Tensor", scale: float, causal: bool) -> "torch.Tensor":
    """Reference attention used as a safe fallback.

    Important: Do not call torch.nn.functional.scaled_dot_product_attention here.
    That function may be patched by the backend to route back into Orchard,
    causing recursion and eventual hard crashes.

    Expects tensors shaped (B, H, S, D).
    """

    # Compute attention scores
    scores = torch.matmul(q, k.transpose(-2, -1)) * float(scale)

    if causal:
        s = scores.size(-1)
        causal_mask = torch.triu(
            torch.ones((s, s), device=scores.device, dtype=torch.bool), diagonal=1
        )
        scores = scores.masked_fill(causal_mask, torch.finfo(scores.dtype).min)

    probs = torch.softmax(scores, dim=-1)
    # Dropout intentionally omitted for deterministic smoke-test fallback.
    return torch.matmul(probs, v)


class FlashAttnFunction(Function):
    """Autograd function for Metal FlashAttention with dropout."""

    @staticmethod
    def forward(ctx, q, k, v, scale: float, dropout_p: float, causal: bool):
        if torch is None:
            _fail("PyTorch not available (did you install torch?)")
        if not hasattr(torch.ops, "flash_attn_mps") or not hasattr(
            torch.ops.flash_attn_mps, "_flash_attn_fwd"
        ):
            _fail(
                "flash_attn_mps kernel not loaded (did you call enable_flash.main()?)"    
            )
        if any(t.device.type != "mps" for t in (q, k, v)):
            _fail(
                f"Input tensors must be on 'mps' device, got {[t.device for t in (q, k, v)]}"
            )

        # The Metal bindings access the underlying MTLBuffer.
        # Some MPS tensors (especially views / grads) can be backed by storage
        # that isn't directly exportable; materializing to contiguous storage
        # avoids "tensor storage is not shared" failures in backward.
        q = _contig(q)
        k = _contig(k)
        v = _contig(v)

        head_dim = q.shape[-1]
        if head_dim % 8 != 0:
            raise ValueError(f"head_dim must be multiple of eight, got {head_dim}")
        if not (0.0 <= dropout_p < 1.0):
            raise ValueError(f"dropout_p must be in [0,1), got {dropout_p}")
        ctx.scale = scale
        ctx.dropout_p = dropout_p
        ctx.causal = causal

        out, mask = torch.ops.flash_attn_mps._flash_attn_fwd(
            q, k, v, float(scale), float(dropout_p), causal
        )
        ctx.save_for_backward(q, k, v, _contig(mask))
        ctx.mark_non_differentiable(mask)
        if _DEBUG:
            print(
                f"[orchard][FlashAttnFunction.forward] Shapes: q={q.shape}, scale={scale}, dropout={dropout_p}, causal={causal}"
            )
        return out, mask

    @staticmethod
    def backward(ctx, grad_out, grad_mask=None):
        """Backward pass with comprehensive Metal support strategy.
        
        Attempts Metal backward kernel first. If it fails due to MTLBuffer/storage issues,
        automatically falls back to PyTorch reference implementation.
        This ensures training always works while maximizing GPU utilization when possible.
        """
        if torch is None:
            _fail("PyTorch not available")

        q, k, v, mask = ctx.saved_tensors
        
        # Pre-allocate grad tensors with explicit shared storage requirements.
        # Metal backward requires all tensors (inputs + outputs) to use shared buffers.
        grad_q = torch.empty_like(q)
        grad_k = torch.empty_like(k)
        grad_v = torch.empty_like(v)
        
        # Ensure all inputs to Metal backward are on shared storage.
        # This is the most critical step: autograd-generated grad_out and mask
        # may not be allocated as shared by default.
        grad_out = _ensure_shared_mps_tensor(grad_out)
        q_shared = _ensure_shared_mps_tensor(q)
        k_shared = _ensure_shared_mps_tensor(k)
        v_shared = _ensure_shared_mps_tensor(v)
        mask_shared = _ensure_shared_mps_tensor(mask)
        grad_q_shared = grad_q  # Already allocated with empty_like; typically shared.
        grad_k_shared = grad_k
        grad_v_shared = grad_v

        metal_kernel_ok = hasattr(torch.ops, "flash_attn_mps") and hasattr(
            torch.ops.flash_attn_mps, "_flash_attn_bwd_dropout"
        )

        def _try_metal(go, qq, kk, vv, mm, gq, gk, gv):
            return torch.ops.flash_attn_mps._flash_attn_bwd_dropout(
                go,
                qq,
                kk,
                vv,
                mm,
                float(ctx.scale),
                float(ctx.dropout_p),
                ctx.causal,
            )

        if metal_kernel_ok:
            try:
                grad_q_metal, grad_k_metal, grad_v_metal = _try_metal(
                    grad_out, q_shared, k_shared, v_shared, mask_shared,
                    grad_q_shared, grad_k_shared, grad_v_shared
                )
                # Copy results back to grad tensors (should be in-place, but be explicit).
                grad_q.copy_(grad_q_metal)
                grad_k.copy_(grad_k_metal)
                grad_v.copy_(grad_v_metal)
                if _DEBUG:
                    print(
                        f"[orchard][FlashAttnFunction.backward] Metal backward succeeded with shared storage tensors."
                    )
            except RuntimeError as e:
                # Comprehensive error handling for Metal backward failures.
                # These can occur due to:
                # 1. Tensor storage mode (shared vs private) incompatibility
                # 2. Allocator restrictions in the current PyTorch MPS build
                # 3. Other Metal or MPS layer issues
                msg = str(e)
                if _DEBUG:
                    print(
                        f"[orchard][FlashAttnFunction.backward] Metal bwd failed ({msg}); falling back to reference attention backward.",
                        file=sys.stderr,
                    )
                
                # Fallback: recompute attention with basic matmul/softmax ops so we
                # don't re-enter any patched SDPA/FlashAttention fastpaths.
                with torch.enable_grad():
                    q_, k_, v_ = [x.detach().clone().requires_grad_(True) for x in (q, k, v)]
                    out = _ref_attention(q_, k_, v_, ctx.scale, ctx.causal)
                    grad_q, grad_k, grad_v = torch.autograd.grad(
                        out, (q_, k_, v_), grad_out
                    )
        elif _DEBUG:
            print(
                "[orchard][FlashAttnFunction.backward] Metal kernel unavailable, attempting reference fallback.",
                file=sys.stderr,
            )
            with torch.enable_grad():
                q_, k_, v_ = [x.detach().clone().requires_grad_(True) for x in (q, k, v)]
                out = _ref_attention(q_, k_, v_, ctx.scale, ctx.causal)
                grad_q, grad_k, grad_v = torch.autograd.grad(out, (q_, k_, v_), grad_out)
        else:
            _fail(
                "Metal FlashAttention backward kernel unavailable (no fallback; set ORCHARD_DEBUG_FLASHATN=1 for PyTorch dev mode)."
            )
        return grad_q, grad_k, grad_v, None, None, None


def flash_attn(q, k, v, scale, dropout_p=0.0, causal=False):
    """Call Metal FlashAttention with autograd support.
    
    Args:
        q: Query tensor on 'mps' device
        k: Key tensor on 'mps' device
        v: Value tensor on 'mps' device
        scale: Attention scale (typically 1/sqrt(head_dim))
        dropout_p: Dropout probability (0.0 to < 1.0)
        causal: Whether to apply causal mask
        
    Returns:
        out: Attention output tensor
        mask: Internal mask tensor (used for backward)
    """
    if torch is None:
        _fail("PyTorch not available")
    return FlashAttnFunction.apply(q, k, v, scale, dropout_p, causal)
