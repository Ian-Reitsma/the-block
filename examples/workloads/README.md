# Workload Examples

Sample workload descriptors demonstrating capability requirements.

- `cpu_only.json` – requests 8 CPU cores and no GPU.
- `gpu_inference.json` – single RTX4090 with 16 GB VRAM.
- `multi_gpu.json` – two A100 GPUs, 32 GB total VRAM.

Run an example with:

```bash
cargo run --example run_workload <path/to/file>
```
