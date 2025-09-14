# Supply Chain Security

This project enables reproducible builds via Nix. The `nix` directory contains a `build.nix` that pins dependencies and produces deterministic binaries. Operators can compare hashes using `nix-build` on multiple machines.

Each release generates a CycloneDX SBOM and container images are signed with `cosign`. CI runs `cargo audit` and `honggfuzz` to detect dependency issues and protocol bugs.

To verify artifacts independently, rebuild using Nix and compare the SHA256 of the produced binaries with the published values. The `scripts/verify_image.sh` script checks that signed images match the expected digest, allowing third parties to validate releases without trusting the CI.
