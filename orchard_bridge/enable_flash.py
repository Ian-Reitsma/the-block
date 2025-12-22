# orchard_bridge/enable_flash.py

"""
Enable Metal FlashAttention for Orchard with starcoder.

This enhances the original Orchard enable_flash with:
- Proper runtime flag propagation (USE_FLASH_ATTN, etc.)
- Graceful fallback and error handling
- Integration with device_backend.py
- Clear diagnostics

Based on: ~/projects/Apple-Metal-Orchard/experimental/orchard_ops/enable_flash.py
"""

import logging
import os
import sys
from pathlib import Path
from typing import Optional

try:
    import torch
except Exception:
    torch = None

logger = logging.getLogger(__name__)


def _find_orchard_dylib():
    """Find libflash_attn.dylib.

    Public-repo safe search order:
    1. ORCHARD_DYLIB_PATH env var (explicit file)
    2. ORCHARD_REPO_PATH env var (repo root containing experimental/)
    3. REPO_ROOT / STARCODER_REPO_ROOT env var (git-starcoder repo root)
    4. Repo-relative default: <git-starcoder>/metal-backend/experimental/...

    Returns:
        Optional[str]: path to dylib
    """
    # 1) Explicit override
    p = os.environ.get("ORCHARD_DYLIB_PATH")
    if p:
        return p

    # Helper: check a candidate repo root
    def _candidate(repo_root: Path) -> Optional[str]:
        dylib = repo_root / "experimental" / "kernel_lib" / "flashattn" / "libflash_attn.dylib"
        return str(dylib) if dylib.exists() else None

    # 2) Explicit Orchard repo root
    repo_env = os.environ.get("ORCHARD_REPO_PATH")
    if repo_env:
        hit = _candidate(Path(repo_env).expanduser().resolve())
        if hit:
            return hit

    # 3) git-starcoder repo root
    star_root = os.environ.get("REPO_ROOT") or os.environ.get("STARCODER_REPO_ROOT")
    if star_root:
        mb = Path(star_root).expanduser().resolve() / "metal-backend"
        hit = _candidate(mb)
        if hit:
            return hit

    # 4) Repo-relative default from this file location
    # enable_flash.py is <git-starcoder>/orchard_bridge/enable_flash.py
    # so git root is parent of orchard_bridge.
    git_root = Path(__file__).resolve().parent.parent
    mb = git_root / "metal-backend"
    hit = _candidate(mb)
    if hit:
        return hit

    return None



def main(verbose: bool = False, strict: bool = False):
    """Load Metal FlashAttention library and register kernels.
    
    Args:
        verbose: Print detailed diagnostics
        strict: Raise exception if dylib not found (vs warn and continue)
        
    Returns:
        bool: True if successfully loaded, False otherwise
        
    Raises:
        RuntimeError: If strict=True and dylib not found
    """
    log_path = os.path.join(
        os.path.dirname(__file__), "orchard_enable_flash.log"
    )
    
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s:%(name)s:%(message)s",
        filename=log_path,
        filemode="a"
    )

    try:
        logger.info("[starcoder-orchard] enable_flash invoked")
        
        if verbose:
            logger.info(f"[starcoder-orchard] running from: {os.path.dirname(__file__)}")
            logger.info(f"[starcoder-orchard] verbose mode enabled")
        
        if torch is None:
            msg = "PyTorch not available"
            logger.warning(f"[starcoder-orchard] {msg}")
            if strict:
                raise RuntimeError(msg)
            return False
        
        # Find dylib
        lib_path = _find_orchard_dylib()
        
        if lib_path is None:
            msg = (
                "libflash_attn.dylib not found. "
                "Set ORCHARD_DYLIB_PATH (file) or ORCHARD_REPO_PATH (repo root), "
                "or place Apple-Metal-Orchard at <repo>/metal-backend."
            )
            logger.warning(f"[starcoder-orchard] {msg}")
            if strict:
                raise RuntimeError(msg)
            if verbose:
                print(f"[starcoder-orchard] WARNING: {msg}", file=sys.stderr)
            return False
        
        # Set Orchard runtime flags
        # USE_FLASH_ATTN=2 tells Orchard to use the experimental bridge
        os.environ["USE_FLASH_ATTN"] = "2"
        
        # Load dylib
        dylib_loaded = False
        try:
            torch.ops.load_library(lib_path)
            dylib_loaded = True
            logger.info(f"[starcoder-orchard] Loaded library: {lib_path}")
            if verbose:
                print(f"[starcoder-orchard] Metal FlashAttention dylib loaded", file=sys.stderr)
        except Exception as e:
            msg = f"Failed to load dylib {lib_path}: {e}"
            logger.warning(f"[starcoder-orchard] {msg}")
            if strict:
                raise RuntimeError(msg)
            if verbose:
                print(f"[starcoder-orchard] WARNING: {msg}", file=sys.stderr)
            return False
        
        # Verify kernels loaded
        kernels_available = (
            hasattr(torch.ops, "flash_attn_mps")
            and hasattr(torch.ops.flash_attn_mps, "_flash_attn_fwd")
            and hasattr(torch.ops.flash_attn_mps, "_flash_attn_bwd_dropout")
        )
        
        if not kernels_available:
            msg = "Metal FlashAttention kernels not registered after dylib load"
            logger.warning(f"[starcoder-orchard] {msg}")
            if strict:
                raise RuntimeError(msg)
            return False
        
        logger.info(
            f"[starcoder-orchard] Metal FlashAttention fully initialized. "
            f"Dylib: {dylib_loaded}, Kernels: {kernels_available}, "
            f"USE_FLASH_ATTN={os.environ.get('USE_FLASH_ATTN')}"
        )
        
        if verbose:
            print(
                f"[starcoder-orchard] Metal FlashAttention ready. "
                f"USE_FLASH_ATTN=2 (set by enable_flash)",
                file=sys.stderr
            )
        
        return True
        
    except Exception as e:
        msg = f"Unexpected error during Metal FlashAttention initialization: {e}"
        logger.error(f"[starcoder-orchard] {msg}", exc_info=True)
        if strict:
            raise
        return False


if __name__ == "__main__":
    verbose = "--verbose" in sys.argv
    strict = "--strict" in sys.argv
    
    try:
        success = main(verbose=verbose, strict=strict)
        sys.exit(0 if success else 1)
    except Exception as e:
        print(f"[starcoder-orchard] FATAL: {e}", file=sys.stderr)
        sys.exit(2)
