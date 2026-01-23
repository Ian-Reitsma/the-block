#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Apple‑Metal Orchard Benchmark Script — Baseline v0.7 → v0.8
Updated: 2025‑07‑14
- Unified device selection (MPS, CUDA, CPU fallback)
- Deterministic seed for cross-device comparison
- Full audit trail: errors, progress, and all output split for robust run reproducibility
- Compatible with new run_epoch.sh logging discipline
- Output logs with flush safety (10s granularity)
- Designed for use with both Apple Metal and CUDA/CPU baselines


Date: 2025‑06‑28
-------------
* **Config YAML** – fully functional `--config path.yaml`; keys override CLI for CI grid‑runs.
* **Large‑file mmap** when corpus > 256 MB avoids RAM blow‑out.
* **Timeline sidecar** – if > 500 points, dumped to `runs/<TAG>_timeline.json`; JSON keeps reference only.
* **GPU die temperature** merged from powermetrics for throttle insight.
* All prior security, audit, determinism, and bandwidth checks retained.
* Harness pointer: see `benchmarks/run_epoch.sh` for env wiring.
"""

import os, sys, json, time, hashlib, argparse, random, re, subprocess, pathlib, mmap
import numpy as np
import torch
import mlx  # MLX kernels first – forces Apple‑patched runtime
import orchard_patch_flash 
from transformers import GPT2TokenizerFast, GPT2LMHeadModel

try:
    import yaml
except ImportError:
    yaml = None  # config optional

import torch
print("MPS available:", torch.backends.mps.is_available(),
      "MPS built:", torch.backends.mps.is_built(), file=sys.stderr)

# ── Environment / path hygiene ───────────────────────────────────────────────
PROJECT_ROOT = pathlib.Path(os.getenv("ORCHARD_ROOT", "~/projects/orchard")).expanduser().resolve()
SETUP_LOG    = PROJECT_ROOT / "setup_log.md"
RUNS_DIR     = PROJECT_ROOT / "runs"
if not SETUP_LOG.is_file():
    sys.exit("[orchard] setup_log.md not found; aborting.")
SCRIPT_START = time.time()

# ── Helper: dataset SHA pulled from setup_log.md ─────────────────────────────

def dataset_sha():
    for line in SETUP_LOG.read_text().splitlines():
        if "wiki.train.tokens" in line:
            return line.split()[0]
    raise RuntimeError("Dataset SHA not found in setup_log.md")

# Metal version detection (capture stderr)
def detect_metal_ver():
    try:
        out = subprocess.check_output(
            ["xcrun", "metal", "-v"],
            env={"LC_ALL": "C"},
            stderr=subprocess.STDOUT,
            text=True
        )
        m = re.search(r"\b\d+\.\d+\b", out)
        return m.group(0) if m else "unknown"
    except Exception:
        return "unknown"

METAL_VER = detect_metal_ver()

# FlashAttention dylib
FLASH_INFO = {"enabled": False}
if os.getenv("USE_FLASH_ATTN") == "1":
    dylib = PROJECT_ROOT / "kernel_lib/flashattn/libflash_attn.dylib"
    import ctypes; ctypes.CDLL(str(dylib))
    sha = hashlib.sha256(dylib.read_bytes()).hexdigest()
    mtime = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(dylib.stat().st_mtime))
    FLASH_INFO = {"enabled": True, "sha": sha, "built": mtime, "metal": METAL_VER}
    print(f"[orchard] FlashAttention loaded (SHA {sha[:8]}…)", file=sys.stderr)

# Reproducibility seed
SEED = int(os.getenv("SEED", 42))
random.seed(SEED); np.random.seed(SEED); torch.manual_seed(SEED)

BF16_ON = os.getenv("USE_BF16") == "1"
DTYPE = torch.bfloat16 if BF16_ON else torch.float32

os.environ["TOKENIZERS_PARALLELISM"] = "false"

# ── Args + optional YAML ────────────────────────────────────────────────────
AP = argparse.ArgumentParser()
AP.add_argument("--data", required=True)
AP.add_argument("--tag",  required=True)
AP.add_argument("--bs",   type=int, default=8)
AP.add_argument("--seq",  type=int, default=512)
AP.add_argument("--steps",type=int, default=100)
AP.add_argument("--config", type=str, help="YAML file overriding any CLI flags")
AP.add_argument("--grad-accum", type=int, default=1, help="Number of gradient accumulation steps (micro-batching)")
args = AP.parse_args()

# merge YAML if provided
if args.config:
    if yaml is None:
        raise RuntimeError("PyYAML missing but --config supplied")
    cfg = yaml.safe_load(pathlib.Path(args.config).read_text())
    for k, v in cfg.items():
        if hasattr(args, k):
            setattr(args, k, v)

DATA_PATH = pathlib.Path(args.data).expanduser().resolve()
if PROJECT_ROOT not in DATA_PATH.parents:
    sys.exit("[orchard] Data path escapes project root — refuse to run.")

# ── Dataset SHA verify ­──────────────────────────────────────────────────────
sha_expected = dataset_sha()
sha_actual = hashlib.sha256(DATA_PATH.read_bytes()).hexdigest()
if sha_actual != sha_expected:
    raise RuntimeError("Dataset SHA mismatch")

# ── Load corpus with mmap fallback ───────────────────────────────────────────
if DATA_PATH.stat().st_size > 256 * 1024 * 1024:  # >256 MB
    print("[orchard] Using mmap load", file=sys.stderr)
    with DATA_PATH.open('r', encoding='utf-8') as f:
        mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        corpus_lines = mm.read().decode('utf-8').splitlines()
else:
    corpus_lines = DATA_PATH.read_text(encoding='utf-8').splitlines()

random.shuffle(corpus_lines)
TOTAL_LINES = len(corpus_lines)

max_steps = min(args.steps, TOTAL_LINES // args.bs)
if max_steps < args.steps:
    print(f"[orchard] Truncated steps to {max_steps} (corpus exhausted)", file=sys.stderr)
args.steps = max_steps

# ── Model & tokenizer ───────────────────────────────────────────────────────
device = torch.device('mps' if torch.backends.mps.is_available() else 'cpu')
model = GPT2LMHeadModel.from_pretrained('gpt2').to(device, dtype=DTYPE)
model.train(); optimizer = torch.optim.AdamW(model.parameters(), lr=5e-5)

tokenizer = GPT2TokenizerFast.from_pretrained('gpt2')
tokenizer.pad_token = tokenizer.eos_token   # allow padding


# Warm‑up
warm = [corpus_lines[0]] * args.bs
for _ in range(2):
    ids = tokenizer(warm, return_tensors='pt', padding='max_length', truncation=True, max_length=args.seq).input_ids.to(device)
    model(ids, labels=ids)

# ── Timed loop ───────────────────────────────────────────────────────────────
start_all = time.time(); tok_s = copy_s = train_s = 0.0
host_mb_total = 0.0; losses=[]; timeline=[]; max_grad=0.0

accum_steps = args.grad_accum
optimizer.zero_grad()

for step in range(args.steps):
    batch = corpus_lines[step*args.bs:(step+1)*args.bs]

    t0=time.time(); enc=tokenizer(batch, return_tensors='pt', padding='max_length', truncation=True, max_length=args.seq); tok_s+=time.time()-t0
    ids=enc.input_ids; host_mb_total+=ids.element_size()*ids.numel()/1e6
    t0=time.time(); ids=ids.to(device); copy_s+=time.time()-t0
    t0=time.time(); out=model(ids,labels=ids); losses.append(out.loss.item())
    # Scale loss for gradient accumulation
    (out.loss / accum_steps).backward()
    for p in model.parameters():
        if p.grad is not None:
            g = p.grad.detach().abs().max().item()
            if not np.isfinite(g): raise RuntimeError("NaN/Inf gradient")
            max_grad = max(max_grad, g)
    # Only step optimizer every accum_steps
    if (step + 1) % accum_steps == 0 or (step + 1) == args.steps:
        optimizer.step()
        optimizer.zero_grad()
    # ── heartbeat every N seconds ─────
    hb_sec = int(os.getenv("PROGRESS_SEC", "10"))
    now = time.time()
    if now - globals().get("_last_hb", 0) >= hb_sec or step == args.steps - 1:
        _last_hb = now                # update global sentinel
        pct = (step + 1) / args.steps
        tps = ids.numel() * (step + 1) / (now - start_all)
        print(f"[orchard][{args.tag}] step {step+1}/{args.steps} "
              f"({pct:5.1%})  wall={now - start_all:6.1f}s  tps≈{tps:7.1f}",
              file=sys.stderr, flush=True)


# Copy audit
expected = args.bs * args.seq * ids.element_size() / 1e6 * args.steps
if abs(expected - host_mb_total) / expected > 0.05:
    raise RuntimeError("Host MB mismatch >5%")

# Power merge + GPU temp
pow_path = os.getenv("POW_FILE")
avg_gpu_w = avg_gpu_temp = None
if pow_path:
    pf = pathlib.Path(pow_path)
    st = pf.stat()
    # Accept root-owned files; just ensure the log was written *during* the run
    if st.st_mtime < SCRIPT_START:
        raise RuntimeError("Power log timestamp predates benchmark start")

    watts, temps = [], []
    for ln in pf.read_text().splitlines():
        if "GPU Power" in ln:          watts.append(float(ln.split()[2]))
        if "GPU Die Temperature" in ln: temps.append(float(ln.split()[3]))
    if watts: avg_gpu_w = sum(watts) / len(watts)
    if temps: avg_gpu_temp = sum(temps) / len(temps)

# Sanitize git commit
raw_commit = subprocess.check_output(['git','rev-parse','--short','HEAD'], env={"LC_ALL":"C"}, text=True).strip()
commit_hex = re.match(r"^[0-9a-f]{7,40}$", raw_commit).group(0)

wall = time.time()-start_all
bw_mb_s = host_mb_total/copy_s if copy_s else None

# Timeline sidecar dumping
timeline_ref = timeline
if len(timeline) > 500:
    RUNS_DIR.mkdir(exist_ok=True)
    sidecar = RUNS_DIR / f"{args.tag}_timeline.json"
    sidecar.write_text(json.dumps(timeline))
    timeline_ref = str(sidecar.relative_to(PROJECT_ROOT))

summary = {
    'schema':'orchard_bench_v1',
    'tag':args.tag,'commit':commit_hex,
    'dataset_sha':sha_actual,
    'batch':args.bs,'seq':args.seq,'micro_steps':args.steps,
    'total_tokens':args.bs*args.seq*args.steps,
    'throughput_tps':(args.bs*args.seq*args.steps)/wall,
    'tokenize_s':round(tok_s,4),
    'copy_s':round(copy_s,4),
    'copy_mb_s':round(bw_mb_s,2) if bw_mb_s else None,
    'train_s':round(train_s,4),
    'host_to_gpu_mb':round(host_mb_total,2),
    'avg_loss_stub':sum(losses)/len(losses),
    'max_grad':max_grad,
    'dtype':str(DTYPE),
    'bf16':BF16_ON,
    'flash':FLASH_INFO,
    'metal_ver':METAL_VER,
    'seed':SEED,
    'timeline':timeline_ref,
    'avg_gpu_w':avg_gpu_w,
    'grad_accum': args.grad_accum,
    'avg_gpu_temp':avg_gpu_temp
}

print("JSON:" + json.dumps(summary))