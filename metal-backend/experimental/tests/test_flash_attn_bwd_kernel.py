import pytest
import torch
import orchard_ops.enable_flash as enable_flash


@pytest.mark.skipif(not torch.backends.mps.is_available(), reason="MPS not available")
@pytest.mark.parametrize("dropout_p", [0.0, 0.5])
@pytest.mark.parametrize("head_dim", [8, 16])
@pytest.mark.parametrize("dtype", [torch.float32, torch.bfloat16])
def test_gradients_match_expected(dropout_p, head_dim, dtype):
    enable_flash.main()
    if not hasattr(enable_flash, "flash_attn"):
        pytest.skip("flash_attn not registered")
    device = torch.device("mps")
    q = torch.randn(2, 4, head_dim, device=device, dtype=dtype, requires_grad=True)
    k = q.clone().detach().requires_grad_()
    v = q.clone().detach().requires_grad_()
    torch.manual_seed(0)
    out, mask = enable_flash.flash_attn(q, k, v, 1.0, dropout_p, False)
    grad_out = torch.randn_like(out)
    out.backward(grad_out)
    expected = grad_out * mask / (1.0 - dropout_p)
    assert torch.allclose(q.grad, expected * 1.0, atol=1e-5)
    assert torch.allclose(k.grad, expected * 1.0, atol=1e-5)
    assert torch.allclose(v.grad, expected, atol=1e-5)


@pytest.mark.skipif(not torch.backends.mps.is_available(), reason="MPS not available")
def test_invalid_params_raise():
    enable_flash.main()
    if not hasattr(enable_flash, "flash_attn"):
        pytest.skip("flash_attn not registered")
    device = torch.device("mps")
    q = torch.randn(2, 4, 7, device=device, requires_grad=True)
    k = q.clone().detach().requires_grad_()
    v = q.clone().detach().requires_grad_()
    with pytest.raises(ValueError):
        enable_flash.flash_attn(q, k, v, 1.0, 0.0, False)
    q_valid = torch.randn(2, 4, 8, device=device, requires_grad=True)
    k_valid = q_valid.clone().detach().requires_grad_()
    v_valid = q_valid.clone().detach().requires_grad_()
    with pytest.raises(ValueError):
        enable_flash.flash_attn(q_valid, k_valid, v_valid, 1.0, -0.1, False)
