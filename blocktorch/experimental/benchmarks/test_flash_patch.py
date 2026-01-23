import os, sys, torch

os.environ["USE_FLASH_ATTN"] = "1"
os.environ["FLASHATTN_LOG_LEVEL"] = "2"
KERNEL_LOG_PATH = "/tmp/flashattn_kernel_calls.log"
if os.path.exists(KERNEL_LOG_PATH):
    os.remove(KERNEL_LOG_PATH)

sys.path.insert(0, os.path.abspath("benchmarks"))
import orchard_patch_flash  # monkeypatches GPT2Attention.forward

from transformers import GPT2Config
from transformers.models.gpt2.modeling_gpt2 import GPT2Attention

cfg = GPT2Config(n_embd=64, n_head=4, n_layer=2)
attn = GPT2Attention(cfg)

print("\n--- TEST: Invalid Device (cpu) ---")
hidden_states = torch.randn(2, 4, 64, device="mps", dtype=torch.float32)
try:
    attn(hidden_states)
except Exception as e:
    print("Expected kernel error (should see fallback logs):", e)

print("\n--- KERNEL LOG FILE ---")
if os.path.exists(KERNEL_LOG_PATH):
    with open(KERNEL_LOG_PATH, "r") as f:
        print(f.read())
else:
    print("(No kernel log file was written, fallback or error path hit.)")

print("\n[COMPLETE] If you see ERROR and Fallback lines above (and kernel_call log lines in /tmp/flashattn_kernel_calls.log), patch is correct and robust.")
