# Data

Place untracked datasets and token files here when running the experimental
PyTorch path. This directory is ignored by Git to keep large binaries out of the
repository and to avoid polluting commits with transient data.

## Next Steps
Provide download scripts or links here if specific datasets become required for reproducible experiments. Document dataset versions, preprocessing steps, and intended use in a README within each subdirectory. Remove the directory when the experimental path is retired.

## Contributor Protocol
- Consult `../../AGENTS.md` for repository rules.
- Do not commit datasets or generated files; this directory stays untracked except for explanatory README files.
- Document dataset origin, version, and preprocessing using inline code for commands and flags; avoid fenced code blocks.
- Prior to submitting changes elsewhere, run `cmake -S . -B build -G Ninja` and `cmake --build build --target check` from the repository root and capture the output.
- Use `-DFETCHCONTENT_FULLY_DISCONNECTED=ON` during configuration when offline so the test suite links against the trimmed `third_party/googletest` tree or a system installation.
- Keep commits single-purpose with an imperative summary line and cite paths and line numbers in the pull request description.
- Use `rg` for repository-wide searches and work only on the default branch.
- When profiling behaviour is exercised, call `tensor_profile_reset` after
  toggling `ORCHARD_TENSOR_PROFILE` and clear `/tmp/orchard_tensor_profile.log`
  with `tensor_profile_clear_log`.
