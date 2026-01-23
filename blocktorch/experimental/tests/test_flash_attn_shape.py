import importlib
import pytest

def torch_available():
    return importlib.util.find_spec('torch') is not None

@pytest.mark.skipif(
    not torch_available() or not __import__('torch').backends.mps.is_available(),
    reason="PyTorch or MPS backend not available"
)
def test_flash_attn_output_shape_and_dtype():
    import orchard_ops.enable_flash as enable_flash
    enable_flash.main(verbose=True)
    if not hasattr(enable_flash, "flash_attn"):
        pytest.skip("FlashAttention not registered; dylib not loaded")

    import torch
    q = torch.randn(2, 4, 64, device='mps', dtype=torch.float16)
    k = torch.randn(2, 4, 64, device='mps', dtype=torch.float16)
    v = torch.randn(2, 4, 64, device='mps', dtype=torch.float16)
    out, mask = enable_flash.flash_attn(q, k, v, 1.0, 0.0, False)
    assert out.shape == q.shape, f"Expected output shape {q.shape}, got {out.shape}"
    assert out.dtype == q.dtype, f"Expected output dtype {q.dtype}, got {out.dtype}"
    assert out.device == q.device, f"Expected output device {q.device}, got {out.device}"
    assert mask.shape == out.shape

