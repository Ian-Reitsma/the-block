# Dependency Inventory

> **Snapshot (2025-10-11):** Regenerated after introducing the shared
> `http_env` helpers and migrating every HTTPS client to the first-party
> `httpd::TlsConnector`. The workspace no longer depends on `native-tls`; TLS
> traffic flows through `foundation_tls` and the vendored `rustls` stack
> recorded below. Use the shared environment prefixes (`TB_*_TLS` or
> service-specific overrides) together with `<PREFIX>_CERT`, `<PREFIX>_KEY`,
> `<PREFIX>_CA`, and `<PREFIX>_INSECURE` plus the `contract tls convert`
> subcommand to populate trust material in the new client wrappers.

| Tier | Crate | Version | Origin | License | Depth |
| --- | --- | --- | --- | --- | --- |
| strategic | `rustls` | 0.21.12 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| replaceable | `serde` | 1.0.228 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `ahash` | 0.8.12 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `aho-corasick` | 1.1.3 | crates.io | Unlicense OR MIT | 3 |
| unclassified | `allocator-api2` | 0.2.21 | crates.io | MIT OR Apache-2.0 | 7 |
| unclassified | `anstyle` | 1.0.13 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `anyhow` | 1.0.100 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `approx` | 0.5.1 | crates.io | Apache-2.0 | 2 |
| unclassified | `asn1-rs` | 0.6.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `asn1-rs-derive` | 0.5.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `asn1-rs-impl` | 0.2.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `assert_cmd` | 2.0.17 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `autocfg` | 1.5.0 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `base64_fp` | 0.1.0 | workspace | — | 1 |
| unclassified | `bitflags` | 2.9.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `bridges` | 0.1.0 | workspace | — | 1 |
| unclassified | `bstr` | 1.12.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `bumpalo` | 3.19.0 | crates.io | MIT OR Apache-2.0 | 7 |
| unclassified | `cc` | 1.2.41 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `cfg-if` | 1.0.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `cli_core` | 0.1.0 | workspace | MIT OR Apache-2.0 | 1 |
| unclassified | `codec` | 0.1.0 | workspace | — | 1 |
| unclassified | `coding` | 0.1.0 | workspace | — | 1 |
| unclassified | `concurrency` | 0.1.0 | workspace | Apache-2.0 | 1 |
| unclassified | `crossbeam-deque` | 0.8.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-epoch` | 0.9.18 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `crossbeam-utils` | 0.8.21 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `crypto` | 0.1.0 | workspace | — | 1 |
| unclassified | `crypto_suite` | 0.1.0 | workspace | — | 1 |
| unclassified | `csv` | 1.3.1 | crates.io | Unlicense/MIT | 2 |
| unclassified | `csv-core` | 0.1.12 | crates.io | Unlicense/MIT | 3 |
| unclassified | `data-encoding` | 2.9.0 | crates.io | MIT | 2 |
| unclassified | `dependency_guard` | 0.1.0 | workspace | MIT | 1 |
| unclassified | `der-parser` | 9.0.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `deranged` | 0.5.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `dex` | 0.1.0 | workspace | — | 1 |
| unclassified | `diagnostics` | 0.1.0 | workspace | Apache-2.0 | 1 |
| unclassified | `difflib` | 0.4.0 | crates.io | MIT | 2 |
| unclassified | `displaydoc` | 0.2.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `dkg` | 0.1.0 | workspace | — | 1 |
| unclassified | `doc-comment` | 0.3.3 | crates.io | MIT | 2 |
| unclassified | `dunce` | 1.0.5 | crates.io | CC0-1.0 OR MIT-0 OR Apache-2.0 | 4 |
| unclassified | `equivalent` | 1.0.2 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `explorer` | 0.1.0 | workspace | — | 2 |
| unclassified | `fallible-iterator` | 0.3.0 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `fallible-streaming-iterator` | 0.1.9 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `find-msvc-tools` | 0.1.4 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `float-cmp` | 0.10.0 | crates.io | MIT | 2 |
| unclassified | `foundation_archive` | 0.1.0 | workspace | — | 1 |
| unclassified | `foundation_lazy` | 0.1.0 | workspace | MIT OR Apache-2.0 | 2 |
| unclassified | `foundation_math` | 0.1.0 | workspace | MIT OR Apache-2.0 | 1 |
| unclassified | `foundation_profiler` | 0.1.0 | workspace | — | 1 |
| unclassified | `foundation_regex` | 0.1.0 | workspace | Apache-2.0 | 1 |
| unclassified | `foundation_rpc` | 0.1.0 | workspace | Apache-2.0 | 1 |
| unclassified | `foundation_serialization` | 0.1.0 | workspace | MIT | 1 |
| unclassified | `foundation_sqlite` | 0.1.0 | workspace | — | 3 |
| unclassified | `foundation_telemetry` | 0.1.0 | workspace | MIT | 1 |
| unclassified | `foundation_time` | 0.1.0 | workspace | — | 1 |
| unclassified | `foundation_tls` | 0.1.0 | workspace | — | 1 |
| unclassified | `foundation_tui` | 0.1.0 | workspace | — | 1 |
| unclassified | `foundation_unicode` | 0.1.0 | workspace | — | 1 |
| unclassified | `futures` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `futures-channel` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-core` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-executor` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-io` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-macro` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `futures-sink` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-task` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-util` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `getrandom` | 0.2.16 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `getrandom` | 0.3.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `glob` | 0.3.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `governance` | 0.1.0 | workspace | — | 1 |
| unclassified | `hashbrown` | 0.14.5 | crates.io | MIT OR Apache-2.0 | 6 |
| unclassified | `hashbrown` | 0.16.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `hashlink` | 0.8.4 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `hidapi` | 2.6.3 | crates.io | MIT | 2 |
| unclassified | `histogram_fp` | 0.1.0 | workspace | — | 1 |
| unclassified | `http_env` | 0.1.0 | workspace | Apache-2.0 | 1 |
| unclassified | `httpd` | 0.1.0 | workspace | — | 1 |
| unclassified | `indexmap` | 2.11.4 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `inflation` | 0.1.0 | workspace | — | 1 |
| unclassified | `itoa` | 1.0.15 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `jobserver` | 0.1.34 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `js-sys` | 0.3.81 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `jurisdiction` | 0.1.0 | workspace | — | 1 |
| unclassified | `lazy_static` | 1.5.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ledger` | 0.1.0 | workspace | — | 1 |
| unclassified | `libc` | 0.2.177 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `libsqlite3-sys` | 0.27.0 | crates.io | MIT | 5 |
| unclassified | `light-client` | 0.1.0 | workspace | — | 1 |
| unclassified | `log` | 0.4.28 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `memchr` | 2.7.6 | crates.io | Unlicense OR MIT | 3 |
| unclassified | `metrics` | 0.21.1 | crates.io | MIT | 2 |
| unclassified | `metrics-macros` | 0.7.1 | crates.io | MIT | 3 |
| unclassified | `minimal-lexical` | 0.2.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `mio` | 0.8.11 | crates.io | MIT | 2 |
| unclassified | `nix` | 0.27.1 | crates.io | MIT | 2 |
| unclassified | `nom` | 7.1.3 | crates.io | MIT | 2 |
| unclassified | `normalize-line-endings` | 0.3.0 | crates.io | Apache-2.0 | 2 |
| unclassified | `num-bigint` | 0.4.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-conv` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `num-integer` | 0.1.46 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `num-traits` | 0.2.19 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `oid-registry` | 0.7.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `once_cell` | 1.21.3 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `p2p_overlay` | 0.1.0 | workspace | — | 1 |
| unclassified | `pin-project` | 1.1.10 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `pin-project-internal` | 1.1.10 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `pin-project-lite` | 0.2.16 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `pin-utils` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `pkg-config` | 0.3.32 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `portable-atomic` | 1.11.1 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `powerfmt` | 0.2.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `pqcrypto-dilithium` | 0.5.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pqcrypto-internals` | 0.2.11 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `pqcrypto-traits` | 0.3.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `predicates` | 3.1.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `predicates-core` | 1.0.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `predicates-tree` | 1.0.12 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `probe` | 0.1.0 | workspace | — | 0 |
| unclassified | `proc-macro2` | 1.0.101 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `quote` | 1.0.41 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `r-efi` | 5.3.0 | crates.io | MIT OR Apache-2.0 OR LGPL-2.1-or-later | 4 |
| unclassified | `rand` | 0.1.0 | workspace | — | 1 |
| unclassified | `rand_core` | 0.1.0 | workspace | — | 2 |
| unclassified | `regex` | 1.11.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `regex-automata` | 0.4.11 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `regex-syntax` | 0.8.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `ring` | 0.17.14 | crates.io | Apache-2.0 AND ISC | 3 |
| unclassified | `runtime` | 0.1.0 | workspace | — | 1 |
| unclassified | `rusqlite` | 0.30.0 | crates.io | MIT | 4 |
| unclassified | `rusticata-macros` | 4.1.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `rustls-webpki` | 0.101.7 | crates.io | ISC | 3 |
| unclassified | `rustversion` | 1.0.22 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `ryu` | 1.0.20 | crates.io | Apache-2.0 OR BSL-1.0 | 3 |
| unclassified | `sct` | 0.7.1 | crates.io | Apache-2.0 OR ISC OR MIT | 3 |
| unclassified | `serde_bytes` | 0.11.19 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `serde_core` | 1.0.228 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_derive` | 1.0.228 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_yaml` | 0.9.34+deprecated | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `shlex` | 1.3.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `slab` | 0.4.11 | crates.io | MIT | 3 |
| unclassified | `sled` | 0.34.0 | workspace | — | 1 |
| unclassified | `smallvec` | 1.15.1 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `socket2` | 0.5.10 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `state` | 0.1.0 | workspace | — | 0 |
| unclassified | `static_assertions` | 1.1.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `storage` | 0.1.0 | workspace | — | 1 |
| unclassified | `storage_engine` | 0.1.0 | workspace | — | 1 |
| unclassified | `syn` | 2.0.106 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `synstructure` | 0.13.2 | crates.io | MIT | 4 |
| unclassified | `sys` | 0.1.0 | workspace | — | 1 |
| unclassified | `tb-sim` | 0.1.0 | workspace | — | 1 |
| unclassified | `termtree` | 0.5.1 | crates.io | MIT | 3 |
| unclassified | `testkit` | 0.1.0 | workspace | — | 1 |
| unclassified | `testkit_macros` | 0.1.0 | workspace | — | 2 |
| unclassified | `the_block` | 0.1.0 | workspace | — | 0 |
| unclassified | `thiserror` | 0.0.1 | workspace | MIT | 1 |
| unclassified | `thiserror` | 1.0.69 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `thiserror-impl` | 1.0.69 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `time` | 0.3.44 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `time-core` | 0.1.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `time-macros` | 0.2.24 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `tracing` | 0.1.41 | crates.io | MIT | 2 |
| unclassified | `tracing-attributes` | 0.1.30 | crates.io | MIT | 3 |
| unclassified | `tracing-core` | 0.1.34 | crates.io | MIT | 3 |
| unclassified | `unicode-ident` | 1.0.19 | crates.io | (MIT OR Apache-2.0) AND Unicode-3.0 | 4 |
| unclassified | `unsafe-libyaml` | 0.2.11 | crates.io | MIT | 3 |
| unclassified | `untrusted` | 0.9.0 | crates.io | ISC | 4 |
| unclassified | `uuid` | 1.18.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `vcpkg` | 0.2.15 | crates.io | MIT/Apache-2.0 | 6 |
| unclassified | `version_check` | 0.9.5 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `wait-timeout` | 0.2.1 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `wallet` | 0.1.0 | workspace | — | 1 |
| unclassified | `wasi` | 0.11.1+wasi-snapshot-preview1 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `wasi` | 0.14.7+wasi-0.2.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 4 |
| unclassified | `wasip2` | 1.0.1+wasi-0.2.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 5 |
| unclassified | `wasm-bindgen` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `wasm-bindgen-backend` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 6 |
| unclassified | `wasm-bindgen-macro` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `wasm-bindgen-macro-support` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `wasm-bindgen-shared` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-sys` | 0.48.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-sys` | 0.52.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-targets` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-targets` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_aarch64_gnullvm` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_aarch64_gnullvm` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_aarch64_msvc` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_aarch64_msvc` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_gnu` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_gnu` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_gnullvm` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_msvc` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_msvc` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnu` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnu` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnullvm` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnullvm` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_msvc` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_msvc` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `wit-bindgen` | 0.46.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 6 |
| unclassified | `x509-parser` | 0.16.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `xtask` | 0.1.0 | workspace | — | 0 |
| unclassified | `zerocopy` | 0.8.27 | crates.io | BSD-2-Clause OR Apache-2.0 OR MIT | 4 |
| unclassified | `zerocopy-derive` | 0.8.27 | crates.io | BSD-2-Clause OR Apache-2.0 OR MIT | 5 |
