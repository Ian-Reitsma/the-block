"""Orchard Metal FlashAttention bridge for starcoder.

This package provides a local integration with Orchard's Metal FlashAttention
without requiring external repo dependencies. It interfaces with the prebuilt
libflash_attn.dylib from Apple-Metal-Orchard.

Usage:
    from orchard_bridge import enable_flash, flash_attn
    enable_flash()  # Load dylib and register kernels
    output = flash_attn(q, k, v, scale=0.125, dropout_p=0.0, causal=False)
"""

__version__ = "0.1.0"
__all__ = [
    "enable_flash",
    "flash_attn",
    "FlashAttnFunction",
    "is_metal_flash_attn_available",
]

try:
    from .flash_attn_function import (
        FlashAttnFunction,
        flash_attn,
        _metal_kernel_available,
    )
    from .enable_flash import main as enable_flash
except ImportError as e:
    # Allow module to load even if torch is missing (useful for CI/non-GPU boxes)
    enable_flash = None
    flash_attn = None
    FlashAttnFunction = None
    _metal_kernel_available = None


def is_metal_flash_attn_available() -> bool:
    """Check if Metal FlashAttention kernels are loaded and ready."""
    if _metal_kernel_available is None:
        return False
    try:
        return _metal_kernel_available()
    except Exception:
        return False
