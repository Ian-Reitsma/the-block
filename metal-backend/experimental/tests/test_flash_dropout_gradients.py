import pytest
import torch
import orchard_ops.enable_flash as enable_flash


@pytest.mark.skipif(not torch.backends.mps.is_available(), reason="MPS not available")
def test_dropout_gradients_match_pytorch():
    enable_flash.main()
    if not hasattr(enable_flash, "flash_attn"):
        pytest.skip("flash_attn not registered")
    device = torch.device("mps")
    torch.manual_seed(0)
    q = torch.randn(2, 4, 16, device=device, requires_grad=True)
    k = q.clone().detach().requires_grad_()
    v = q.clone().detach().requires_grad_()
    out, mask = enable_flash.flash_attn(q, k, v, 1.0, 0.5, False)
    grad_out = torch.randn_like(out)
    out.backward(grad_out)
    grad_k, grad_v = k.grad.clone(), v.grad.clone()

    torch.manual_seed(0)
    q_ref = q.detach().clone().requires_grad_(True)
    k_ref = k.detach().clone().requires_grad_(True)
    v_ref = v.detach().clone().requires_grad_(True)
    attn = torch.nn.functional.scaled_dot_product_attention(
        q_ref, k_ref, v_ref, dropout_p=0.0, is_causal=False
    )
    out_ref = mask * attn / (1.0 - 0.5)
    _, grad_k_ref, grad_v_ref = torch.autograd.grad(out_ref, (q_ref, k_ref, v_ref), grad_out)

    assert torch.allclose(grad_k, grad_k_ref, atol=1e-5)
    assert torch.allclose(grad_v, grad_v_ref, atol=1e-5)
