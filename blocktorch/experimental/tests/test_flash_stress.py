import pytest
import torch
import orchard_ops.enable_flash as enable_flash


@pytest.mark.skipif(not torch.backends.mps.is_available(), reason="MPS not available")
@pytest.mark.parametrize("dropout_p", [0.0, 0.5])
@pytest.mark.parametrize("dtype", [torch.float32, torch.bfloat16])
def test_flash_attn_dropout_rates(dropout_p, dtype):
    enable_flash.main()
    if not hasattr(enable_flash, "flash_attn"):
        pytest.skip("flash_attn not registered")
    device = torch.device("mps")
    q = torch.randn(2, 4, 16, device=device, dtype=dtype, requires_grad=True)
    k = q.clone().detach().requires_grad_()
    v = q.clone().detach().requires_grad_()
    torch.manual_seed(0)
    out, mask = enable_flash.flash_attn(q, k, v, 1.0, dropout_p, False)
    grad = torch.randn_like(out)
    out.backward(grad)
    assert q.grad is not None
    assert k.grad is not None
    assert v.grad is not None


@pytest.mark.skipif(not torch.backends.mps.is_available(), reason="MPS not available")
def test_flash_attn_invalid_inputs():
    enable_flash.main()
    if not hasattr(enable_flash, "flash_attn"):
        pytest.skip("flash_attn not registered")
    device = torch.device("mps")
    q = torch.randn(2, 4, 15, device=device, requires_grad=True)
    k = q.clone().detach().requires_grad_()
    v = q.clone().detach().requires_grad_()
    with pytest.raises(ValueError):
        enable_flash.flash_attn(q, k, v, 1.0, 0.0, False)
    q_bad = torch.randn(2, 4, 16, device=device, requires_grad=True)
    k_bad = q_bad.clone().detach().requires_grad_()
    v_bad = q_bad.clone().detach().requires_grad_()
    with pytest.raises(ValueError):
        enable_flash.flash_attn(q_bad, k_bad, v_bad, 1.0, 1.2, False)
