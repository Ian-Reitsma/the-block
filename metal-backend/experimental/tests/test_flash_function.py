import pytest
import torch
import orchard_ops.enable_flash as enable_flash

@pytest.mark.skipif(not torch.backends.mps.is_available(), reason="MPS not available")
def test_flash_attn_gradcheck():
    enable_flash.main()
    if not hasattr(enable_flash, 'flash_attn'):
        pytest.skip('flash_attn not registered')
    device = torch.device('mps')
    q = torch.randn(2, 4, 16, dtype=torch.float32, requires_grad=True, device=device)
    k = q.clone().detach().requires_grad_()
    v = q.clone().detach().requires_grad_()
    func = lambda q, k, v: enable_flash.flash_attn(q, k, v, 1.0, 0.0, False)[0]
    torch.autograd.gradcheck(func, (q.double(), k.double(), v.double()), eps=1e-3, atol=1e-2)

