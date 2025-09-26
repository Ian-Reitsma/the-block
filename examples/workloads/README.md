# Workload Examples
> **Review (2025-09-25):** Synced Workload Examples guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

Sample workload descriptors demonstrating capability requirements.

- `cpu_only.json` – requests 8 CPU cores and no GPU.
- `gpu_inference.json` – single RTX4090 with 16 GB VRAM.
- `multi_gpu.json` – two A100 GPUs, 32 GB total VRAM.
- `tpu_inference.json` – TPU accelerator with 8 GB HBM.
- `fpga_inference.json` – generic FPGA with 2 GB memory.

Run an example with:

```bash
cargo run --example run_workload <path/to/file>
```
