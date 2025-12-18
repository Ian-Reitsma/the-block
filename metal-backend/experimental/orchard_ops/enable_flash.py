# orchard_ops/enable_flash.py

"""
Enable Apple Metal FlashAttention for Orchard.
- Loads Metal FlashAttention dylib if available (logs errors, never crashes).
- Registers FlashAttnFunction as the universal interface (autograd-wrapped).
- Adds runtime-checked .flash_attn dispatcher to this module.
- Designed for robust CI, script, or interactive use.
"""

import logging
import os
import sys

try:
    import torch
except Exception:  # pragma: no cover - torch may not be installed in all envs
    torch = None

try:
    from .flash_attn_function import FlashAttnFunction
except ImportError as e:
    FlashAttnFunction = None

def main(verbose=False):
    log_path = os.path.join(os.path.dirname(__file__), "orchard_debug.log")
    # Set up logging to file only, NOT terminal
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s:%(name)s:%(message)s",
        filename=log_path,
        filemode="a"  # append to existing log
    )
    logger = logging.getLogger("orchard_ops.enable_flash")

    try:
        logger.info("[orchard] enable_flash invoked")
        logger.info(f"[orchard] running from: {os.path.dirname(__file__)}")
        if verbose:
            logger.info("[orchard] Attempting to load Metal FlashAttention library")

        lib_path = os.path.join(
            os.path.dirname(os.path.dirname(__file__)), "kernel_lib", "flashattn", "libflash_attn.dylib"
        )

        dylib_loaded = False
        if torch is not None:
            try:
                torch.ops.load_library(lib_path)
                dylib_loaded = True
                logger.info(f"[orchard] Loaded library: {lib_path}")
            except Exception as e:
                logger.warning(f"[orchard] Failed to load library: {e}")
        else:
            logger.warning("[orchard] torch not available; cannot load library")

        def flash_attn(q, k, v, scale, dropout_p=0.0, causal=False):
            """Universal entry for FlashAttention with autograd. Raises if dependencies unavailable."""
            if torch is None or FlashAttnFunction is None:
                raise RuntimeError("PyTorch and FlashAttnFunction must be available")
            return FlashAttnFunction.apply(q, k, v, scale, dropout_p, causal)

        setattr(sys.modules[__name__], "flash_attn", flash_attn)
        logger.info(f"[orchard] FlashAttention function registered. DYLIB loaded: {dylib_loaded}")

        # For shell debugging or test: print success
        if verbose:
            print(f"[orchard][enable_flash] Metal FlashAttention {'loaded' if dylib_loaded else 'NOT loaded'} (lib: {lib_path})", file=sys.stderr)

    except Exception as e:
        # Print critical errors to the terminal and always flush logs for CI
        print("[orchard][CRITICAL] Error during logging or kernel load:", e, file=sys.stderr)
        print(f"[orchard][CRITICAL] See log for details: {log_path}", file=sys.stderr)
        logger.error(f"[orchard][CRITICAL] Exception: {e}", exc_info=True)
        raise

if __name__ == "__main__":
    verbose = "--verbose" in sys.argv
    try:
        main(verbose=verbose)
    except Exception as e:
        print(f"[orchard][CRITICAL] Fatal error: {e}", file=sys.stderr)
        sys.exit(1)
