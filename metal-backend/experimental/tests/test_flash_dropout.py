import pytest
import torch
import orchard_ops.enable_flash as enable_flash

@pytest.mark.skipif(not torch.backends.mps.is_available(), reason="MPS not available")
def test_flash_attn_dropout_and_grad():
    enable_flash.main()
    if not hasattr(enable_flash, 'flash_attn'):
        pytest.skip('flash_attn not registered')
    device = torch.device('mps')
    q = torch.randn(2, 4, 16, device=device, requires_grad=True)
    k = q.clone().detach().requires_grad_()
    v = q.clone().detach().requires_grad_()
    torch.manual_seed(0)
    out, mask = enable_flash.flash_attn(q, k, v, 1.0, 0.5, False)
    drop_rate = 1 - mask.float().mean().item()
    assert 0.3 < drop_rate < 0.7
    grad = torch.randn_like(out)
    out.backward(grad)
    assert q.grad is not None and k.grad is not None and v.grad is not None

