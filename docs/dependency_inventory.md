# Dependency Inventory
> **Review (2025-10-02):** Highlighted outstanding clap/toml removal ahead of cli_core rollout across toolchains.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25).

| Tier | Crate | Version | Origin | License | Depth |
| --- | --- | --- | --- | --- | --- |
| strategic | `crypto_suite::signatures::ed25519_inhouse` | — | workspace | project-internal | 0 |
| strategic | `crypto_suite::hashing::blake3` | — | workspace | project-internal | 0 |
| strategic | `crypto_suite::hashing::sha3` | — | workspace | project-internal | 0 |
| strategic | `libp2p` | 0.52.4 | crates.io | MIT | 1 |
| strategic | `quinn` | 0.10.2 | crates.io | MIT OR Apache-2.0 | 3 |
| strategic | `rustls` | 0.21.12 | crates.io | Apache-2.0 OR ISC OR MIT | 1 |
| strategic | `rustls` | 0.22.4 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| strategic | `rustls` | 0.23.31 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| strategic | `tokio` | 1.47.1 | crates.io | MIT | 1 |
| replaceable | `bincode` | 1.3.3 | crates.io | MIT | 1 |
| replaceable | `raptorq` | 2.0.0 | crates.io | Apache-2.0 | 1 |
| replaceable | `rocksdb` | 0.21.0 | path | Apache-2.0 | 1 |
| replaceable | `serde` | 1.0.224 | crates.io | MIT OR Apache-2.0 | 1 |
| replaceable | `sled` | 0.34.7 | crates.io | MIT/Apache-2.0 | 1 |
| replaceable | `zstd` | 0.12.4 | crates.io | MIT | 1 |
| replaceable | `zstd` | 0.13.3 | crates.io | MIT | 3 |
| unclassified | `addr2line` | 0.22.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `addr2line` | 0.24.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `adler2` | 2.0.1 | crates.io | 0BSD OR MIT OR Apache-2.0 | 3 |
| unclassified | `aead` | 0.5.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `aes` | 0.8.4 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `aes-gcm` | 0.10.3 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `ahash` | 0.7.8 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `ahash` | 0.8.12 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `aho-corasick` | 1.1.3 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `allocator-api2` | 0.2.21 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `android_system_properties` | 0.1.5 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `anes` | 0.1.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `anstream` | 0.6.20 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `anstyle` | 1.0.11 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `anstyle-parse` | 0.2.7 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `anstyle-query` | 1.1.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `anstyle-wincon` | 3.0.10 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `anyhow` | 1.0.99 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `approx` | 0.5.1 | crates.io | Apache-2.0 | 2 |
| unclassified | `arbitrary` | 1.4.2 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `arrayref` | 0.3.9 | crates.io | BSD-2-Clause | 2 |
| unclassified | `arrayvec` | 0.5.2 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `arrayvec` | 0.7.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ascii` | 1.1.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `ascii-canvas` | 3.0.0 | crates.io | Apache-2.0/MIT | 5 |
| unclassified | `asn1-rs` | 0.5.2 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `asn1-rs` | 0.6.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `asn1-rs-derive` | 0.4.0 | crates.io | MIT/Apache-2.0 | 6 |
| unclassified | `asn1-rs-derive` | 0.5.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `asn1-rs-impl` | 0.1.0 | crates.io | MIT/Apache-2.0 | 6 |
| unclassified | `asn1-rs-impl` | 0.2.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `assert-json-diff` | 2.0.2 | crates.io | MIT | 3 |
| unclassified | `assert_cmd` | 2.0.17 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `async-attributes` | 1.1.2 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `async-channel` | 1.9.0 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `async-channel` | 2.5.0 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `async-executor` | 1.13.3 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `async-global-executor` | 2.4.1 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `async-io` | 2.6.0 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `async-lock` | 3.4.1 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `async-object-pool` | 0.1.5 | crates.io | MIT | 3 |
| unclassified | `async-process` | 2.5.0 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `async-signal` | 0.2.13 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `async-std` | 1.13.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `async-task` | 4.7.1 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `async-trait` | 0.1.89 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `asynchronous-codec` | 0.6.2 | crates.io | MIT | 3 |
| unclassified | `atomic-waker` | 1.1.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `attohttpc` | 0.24.1 | crates.io | MPL-2.0 | 4 |
| unclassified | `autocfg` | 1.5.0 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `aws-lc-rs` | 1.14.0 | crates.io | ISC AND (Apache-2.0 OR ISC) | 3 |
| unclassified | `aws-lc-sys` | 0.31.0 | crates.io | ISC AND (Apache-2.0 OR ISC) AND OpenSSL | 4 |
| unclassified | `axum` | 0.7.9 | crates.io | MIT | 1 |
| unclassified | `axum-core` | 0.4.5 | crates.io | MIT | 2 |
| unclassified | `axum-macros` | 0.4.2 | crates.io | MIT | 2 |
| unclassified | `backtrace` | 0.3.75 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `base-x` | 0.2.11 | crates.io | MIT | 4 |
| unclassified | `base64` | 0.13.1 | crates.io | MIT/Apache-2.0 | 6 |
| unclassified | `base64` | 0.21.7 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `base64` | 0.22.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `base64ct` | 1.8.0 | crates.io | Apache-2.0 OR MIT | 1 |
| unclassified | `basic-cookies` | 0.1.5 | crates.io | MIT | 3 |
| strategic | `crypto_suite::zk::groth16_inhouse` | — | workspace | project-internal | 0 |
| unclassified | `bindgen` | 0.65.1 | crates.io | BSD-3-Clause | 3 |
| unclassified | `bindgen` | 0.72.1 | crates.io | BSD-3-Clause | 4 |
| unclassified | `bit-set` | 0.5.3 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `bit-set` | 0.8.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `bit-vec` | 0.6.3 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `bit-vec` | 0.8.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `bitflags` | 1.3.2 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `bitflags` | 2.9.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `blake2` | 0.10.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `blake2s_const` | 0.8.0 | crates.io | MIT | 2 |
| unclassified | `blake2s_simd` | 0.5.11 | crates.io | MIT | 2 |
| unclassified | `block-buffer` | 0.10.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `blocking` | 1.6.2 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `bridges` | 0.1.0 | workspace | — | 1 |
| unclassified | `bs58` | 0.5.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `bstr` | 1.12.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `bumpalo` | 3.19.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `bytemuck` | 1.23.2 | crates.io | Zlib OR Apache-2.0 OR MIT | 4 |
| unclassified | `byteorder` | 1.5.0 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `bytes` | 1.10.1 | crates.io | MIT | 1 |
| unclassified | `bzip2-sys` | 0.1.13+1.0.8 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `cast` | 0.3.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `cc` | 1.2.37 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `cexpr` | 0.6.0 | crates.io | Apache-2.0/MIT | 4 |
| unclassified | `cfg-if` | 0.1.10 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `cfg-if` | 1.0.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `chacha20` | 0.9.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `chacha20poly1305` | 0.10.1 | crates.io | Apache-2.0 OR MIT | 1 |
| unclassified | `chrono` | 0.4.42 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `chunked_transfer` | 1.5.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ciborium` | 0.2.2 | crates.io | Apache-2.0 | 2 |
| unclassified | `ciborium-io` | 0.2.2 | crates.io | Apache-2.0 | 3 |
| unclassified | `ciborium-ll` | 0.2.2 | crates.io | Apache-2.0 | 3 |
| unclassified | `cipher` | 0.4.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `clang-sys` | 1.8.1 | crates.io | Apache-2.0 | 4 |
| unclassified | `clap` | 4.5.47 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `clap_builder` | 4.5.47 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `clap_complete` | 4.5.57 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `clap_derive` | 4.5.47 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `clap_lex` | 0.7.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `serde` | 1.0.224 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `serde_derive` | 1.0.224 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_json` | 1.0.145 | crates.io | MIT OR Apache-2.0 | 1 |

> **Action:** Replace the clap/serde/bincode/toml surfaces above with `cli_core` + the JSON codec during the CLI/node/tooling migration. Track progress in `docs/roadmap.md#tooling-migrations`.
| unclassified | `cmake` | 0.1.54 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `cobs` | 0.3.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `colorchoice` | 1.0.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `colored` | 2.2.0 | crates.io | MPL-2.0 | 1 |
| unclassified | `concurrent-queue` | 2.5.0 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `console` | 0.15.11 | crates.io | MIT | 2 |
| unclassified | `const-oid` | 0.9.6 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `constant_time_eq` | 0.1.5 | crates.io | CC0-1.0 | 3 |
| unclassified | `constant_time_eq` | 0.3.1 | crates.io | CC0-1.0 OR MIT-0 OR Apache-2.0 | 2 |
| unclassified | `core-foundation` | 0.9.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `core-foundation-sys` | 0.8.7 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `core2` | 0.4.0 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `cpp_demangle` | 0.4.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `cpufeatures` | 0.2.17 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `cranelift-bforest` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 4 |
| unclassified | `cranelift-bitset` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `cranelift-codegen` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `cranelift-codegen-meta` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 4 |
| unclassified | `cranelift-codegen-shared` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 4 |
| unclassified | `cranelift-control` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `cranelift-entity` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `cranelift-frontend` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `cranelift-isle` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 4 |
| unclassified | `cranelift-native` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `cranelift-wasm` | 0.111.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `crc32fast` | 1.5.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `criterion` | 0.5.1 | crates.io | Apache-2.0 OR MIT | 1 |
| unclassified | `criterion-plot` | 0.5.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `crossbeam` | 0.7.3 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `crossbeam` | 0.8.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-channel` | 0.4.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `crossbeam-channel` | 0.5.15 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-deque` | 0.7.4 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `crossbeam-deque` | 0.8.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `crossbeam-epoch` | 0.8.2 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `crossbeam-epoch` | 0.9.18 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-queue` | 0.2.3 | crates.io | MIT/Apache-2.0 AND BSD-2-Clause | 3 |
| unclassified | `crossbeam-queue` | 0.3.12 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `crossbeam-utils` | 0.7.2 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `crossbeam-utils` | 0.8.21 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crunchy` | 0.2.4 | crates.io | MIT | 3 |
| unclassified | `crypto` | 0.1.0 | workspace | — | 1 |
| unclassified | `crypto-common` | 0.1.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `csv` | 1.3.1 | crates.io | Unlicense/MIT | 1 |
| unclassified | `csv-core` | 0.1.12 | crates.io | Unlicense/MIT | 2 |
| unclassified | `ctr` | 0.9.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `curve25519-dalek` | 4.1.3 | crates.io | BSD-3-Clause | 2 |
| unclassified | `curve25519-dalek-derive` | 0.1.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `dashmap` | 5.5.3 | crates.io | MIT | 1 |
| unclassified | `data-encoding` | 2.9.0 | crates.io | MIT | 2 |
| unclassified | `data-encoding-macro` | 0.1.18 | crates.io | MIT | 4 |
| unclassified | `data-encoding-macro-internal` | 0.1.16 | crates.io | MIT | 5 |
| unclassified | `debugid` | 0.8.0 | crates.io | Apache-2.0 | 3 |
| unclassified | `der` | 0.7.10 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `der-parser` | 8.2.0 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `der-parser` | 9.0.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `deranged` | 0.5.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `derive_arbitrary` | 1.4.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `dex` | 0.1.0 | workspace | — | 1 |
| unclassified | `difflib` | 0.4.0 | crates.io | MIT | 2 |
| unclassified | `digest` | 0.10.7 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `directories-next` | 2.0.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `dirs` | 5.0.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `dirs-next` | 2.0.0 | crates.io | MIT OR Apache-2.0 | 6 |
| unclassified | `dirs-sys` | 0.4.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `dirs-sys-next` | 0.1.2 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `displaydoc` | 0.2.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `dkg` | 0.1.0 | workspace | — | 1 |
| unclassified | `doc-comment` | 0.3.3 | crates.io | MIT | 2 |
| unclassified | `dtoa` | 1.0.10 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `dunce` | 1.0.5 | crates.io | CC0-1.0 OR MIT-0 OR Apache-2.0 | 4 |
| unclassified | `ed25519` | 2.2.3 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `either` | 1.15.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `embedded-io` | 0.4.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `embedded-io` | 0.6.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `ena` | 0.14.3 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `encode_unicode` | 1.0.0 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `encoding_rs` | 0.8.35 | crates.io | (Apache-2.0 OR MIT) AND BSD-3-Clause | 2 |
| unclassified | `enum-as-inner` | 0.5.1 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `enum-as-inner` | 0.6.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `env_filter` | 0.1.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `env_logger` | 0.11.8 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `equivalent` | 1.0.2 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `errno` | 0.3.14 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `event-listener` | 2.5.3 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `event-listener` | 5.4.1 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `event-listener-strategy` | 0.5.4 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `explorer` | 0.1.0 | workspace | — | 2 |
| unclassified | `failure` | 0.1.8 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `failure_derive` | 0.1.8 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `fallible-iterator` | 0.3.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `fallible-streaming-iterator` | 0.1.9 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `fastrand` | 2.3.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `ff` | 0.6.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `ff_ce` | 0.14.3 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `ff_derive` | 0.6.0 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `ff_derive_ce` | 0.11.2 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `fiat-crypto` | 0.2.9 | crates.io | MIT OR Apache-2.0 OR BSD-1-Clause | 3 |
| unclassified | `filetime` | 0.2.26 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `find-msvc-tools` | 0.1.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `findshlibs` | 0.10.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `fixedbitset` | 0.4.2 | crates.io | MIT/Apache-2.0 | 6 |
| unclassified | `flate2` | 1.1.2 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `float-cmp` | 0.10.0 | crates.io | MIT | 2 |
| unclassified | `fnv` | 1.0.7 | crates.io | Apache-2.0 / MIT | 2 |
| unclassified | `foldhash` | 0.1.5 | crates.io | Zlib | 3 |
| unclassified | `foreign-types` | 0.3.2 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `foreign-types-shared` | 0.1.1 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `form_urlencoded` | 1.2.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `fs2` | 0.4.3 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `fs_extra` | 1.3.0 | crates.io | MIT | 5 |
| unclassified | `fsevent-sys` | 4.1.0 | crates.io | MIT | 2 |
| unclassified | `fuchsia-cprng` | 0.1.1 | crates.io | file:LICENSE | 3 |
| unclassified | `futures` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `futures-channel` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-core` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-executor` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-io` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-lite` | 2.6.1 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `futures-macro` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `futures-rustls` | 0.24.0 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `futures-sink` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-task` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-timer` | 3.0.3 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `futures-util` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `fxhash` | 0.2.1 | crates.io | Apache-2.0/MIT | 2 |
| unclassified | `fxprof-processed-profile` | 0.6.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `generic-array` | 0.14.7 | crates.io | MIT | 3 |
| unclassified | `getrandom` | 0.1.16 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `getrandom` | 0.2.16 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `getrandom` | 0.3.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ghash` | 0.5.1 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `gimli` | 0.29.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `gimli` | 0.31.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `git2` | 0.18.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `glob` | 0.3.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `globset` | 0.4.16 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `gloo-timers` | 0.3.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `group` | 0.6.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `h2` | 0.3.27 | crates.io | MIT | 2 |
| unclassified | `h2` | 0.4.12 | crates.io | MIT | 2 |
| unclassified | `half` | 1.8.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `half` | 2.6.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `hashbrown` | 0.12.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `hashbrown` | 0.13.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `hashbrown` | 0.14.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hashbrown` | 0.15.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hashlink` | 0.8.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hdrhistogram` | 7.5.4 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `heck` | 0.4.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `heck` | 0.5.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `hermit-abi` | 0.3.9 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `hermit-abi` | 0.5.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hex` | 0.4.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `hex_fmt` | 0.3.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `hidapi` | 2.6.3 | crates.io | MIT | 2 |
| unclassified | `hkdf` | 0.12.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hmac` | 0.12.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `http` | 0.2.12 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `http` | 1.3.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `http-body` | 0.4.6 | crates.io | MIT | 2 |
| unclassified | `http-body` | 1.0.1 | crates.io | MIT | 2 |
| unclassified | `http-body-util` | 0.1.3 | crates.io | MIT | 2 |
| unclassified | `httparse` | 1.10.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `httpdate` | 1.0.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `httpmock` | 0.7.0 | crates.io | MIT | 2 |
| unclassified | `hyper` | 0.14.32 | crates.io | MIT | 2 |
| unclassified | `hyper` | 1.7.0 | crates.io | MIT | 2 |
| unclassified | `hyper-rustls` | 0.24.2 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| unclassified | `hyper-rustls` | 0.27.7 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| unclassified | `hyper-tls` | 0.5.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `hyper-tls` | 0.6.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `hyper-util` | 0.1.17 | crates.io | MIT | 2 |
| unclassified | `iana-time-zone` | 0.1.64 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `iana-time-zone-haiku` | 0.1.2 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `icu_collections` | 2.0.0 | crates.io | Unicode-3.0 | 5 |
| unclassified | `icu_locale_core` | 2.0.0 | crates.io | Unicode-3.0 | 5 |
| unclassified | `icu_normalizer` | 2.0.0 | crates.io | Unicode-3.0 | 4 |
| unclassified | `icu_normalizer_data` | 2.0.0 | crates.io | Unicode-3.0 | 5 |
| unclassified | `icu_properties` | 2.0.1 | crates.io | Unicode-3.0 | 4 |
| unclassified | `icu_properties_data` | 2.0.1 | crates.io | Unicode-3.0 | 5 |
| unclassified | `icu_provider` | 2.0.0 | crates.io | Unicode-3.0 | 5 |
| unclassified | `id-arena` | 2.2.1 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `idna` | 0.2.3 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `idna` | 0.4.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `idna` | 1.1.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `idna_adapter` | 1.2.1 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `if-addrs` | 0.10.2 | crates.io | MIT OR BSD-3-Clause | 4 |
| unclassified | `if-watch` | 3.2.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `igd-next` | 0.14.3 | crates.io | MIT | 3 |
| unclassified | `indexmap` | 2.11.1 | crates.io | Apache-2.0 OR MIT | 1 |
| unclassified | `indoc` | 2.0.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `inferno` | 0.11.21 | crates.io | CDDL-1.0 | 2 |
| unclassified | `inflation` | 0.1.0 | workspace | — | 1 |
| unclassified | `inotify` | 0.9.6 | crates.io | ISC | 2 |
| unclassified | `inotify-sys` | 0.1.5 | crates.io | ISC | 3 |
| unclassified | `inout` | 0.1.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `insta` | 1.43.2 | crates.io | Apache-2.0 | 1 |
| unclassified | `instant` | 0.1.13 | crates.io | BSD-3-Clause | 2 |
| unclassified | `io-lifetimes` | 1.0.11 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `io-uring` | 0.7.10 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ipconfig` | 0.3.2 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `ipnet` | 2.11.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `iri-string` | 0.7.8 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `is-terminal` | 0.4.16 | crates.io | MIT | 2 |
| unclassified | `is_terminal_polyfill` | 1.70.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `itertools` | 0.10.5 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `itertools` | 0.11.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `itertools` | 0.12.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `itoa` | 1.0.15 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ittapi` | 0.4.0 | crates.io | GPL-2.0-only OR BSD-3-Clause | 2 |
| unclassified | `ittapi-sys` | 0.4.0 | crates.io | GPL-2.0-only OR BSD-3-Clause | 3 |
| unclassified | `jiff` | 0.2.15 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `jiff-static` | 0.2.15 | crates.io | Unlicense OR MIT | 3 |
| unclassified | `jobserver` | 0.1.34 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `js-sys` | 0.3.78 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `json` | 0.12.4 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `jsonrpc-core` | 18.0.0 | crates.io | MIT | 1 |
| unclassified | `jurisdiction` | 0.1.0 | workspace | — | 1 |
| unclassified | `keccak` | 0.1.5 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `kqueue` | 1.1.1 | crates.io | MIT | 2 |
| unclassified | `kqueue-sys` | 1.0.4 | crates.io | MIT | 3 |
| unclassified | `kv-log-macro` | 1.0.7 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `lalrpop` | 0.20.2 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `lalrpop-util` | 0.20.2 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `lazy_static` | 1.5.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `lazycell` | 1.3.0 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `leb128` | 0.2.5 | crates.io | Apache-2.0/MIT | 3 |
| unclassified | `leb128fmt` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `ledger` | 0.1.0 | workspace | — | 1 |
| unclassified | `levenshtein` | 1.0.5 | crates.io | MIT | 3 |
| unclassified | `libc` | 0.2.175 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `libgit2-sys` | 0.16.2+1.7.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `libloading` | 0.8.8 | crates.io | ISC | 5 |
| unclassified | `libm` | 0.2.15 | crates.io | MIT | 2 |
| unclassified | `libp2p-allow-block-list` | 0.2.0 | crates.io | MIT | 2 |
| unclassified | `libp2p-connection-limits` | 0.2.1 | crates.io | MIT | 2 |
| unclassified | `libp2p-core` | 0.40.1 | crates.io | MIT | 2 |
| unclassified | `libp2p-dns` | 0.40.1 | crates.io | MIT | 2 |
| unclassified | `libp2p-identity` | 0.2.12 | crates.io | MIT | 2 |
| unclassified | `libp2p-kad` | 0.44.6 | crates.io | MIT | 2 |
| unclassified | `libp2p-mdns` | 0.44.0 | crates.io | MIT | 2 |
| unclassified | `libp2p-metrics` | 0.13.1 | crates.io | MIT | 2 |
| unclassified | `libp2p-noise` | 0.43.2 | crates.io | MIT | 2 |
| unclassified | `libp2p-quic` | 0.9.3 | crates.io | MIT | 2 |
| unclassified | `libp2p-swarm` | 0.43.7 | crates.io | MIT | 2 |
| unclassified | `libp2p-tcp` | 0.40.1 | crates.io | MIT | 2 |
| unclassified | `libp2p-tls` | 0.2.1 | crates.io | MIT | 3 |
| unclassified | `libp2p-upnp` | 0.1.1 | crates.io | MIT | 2 |
| unclassified | `libp2p-yamux` | 0.44.1 | crates.io | MIT | 2 |
| unclassified | `libredox` | 0.1.10 | crates.io | MIT | 3 |
| unclassified | `librocksdb-sys` | 0.11.0+8.1.1 | crates.io | MIT/Apache-2.0/BSD-3-Clause | 2 |
| unclassified | `libsqlite3-sys` | 0.27.0 | crates.io | MIT | 2 |
| unclassified | `libssh2-sys` | 0.3.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `libz-sys` | 1.1.22 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `light-client` | 0.1.0 | workspace | — | 1 |
| unclassified | `linked-hash-map` | 0.5.6 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `linux-raw-sys` | 0.1.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `linux-raw-sys` | 0.11.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `linux-raw-sys` | 0.3.8 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `linux-raw-sys` | 0.4.15 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `litemap` | 0.8.0 | crates.io | Unicode-3.0 | 6 |
| unclassified | `lock_api` | 0.4.13 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `log` | 0.4.28 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `logtest` | 2.0.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `lru` | 0.11.1 | crates.io | MIT | 1 |
| unclassified | `lru` | 0.7.8 | crates.io | MIT | 2 |
| unclassified | `lru-cache` | 0.1.2 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `lz4-sys` | 1.11.1+lz4-1.10.0 | crates.io | MIT | 3 |
| unclassified | `mach2` | 0.4.3 | crates.io | BSD-2-Clause OR MIT OR Apache-2.0 | 2 |
| unclassified | `matchers` | 0.2.0 | crates.io | MIT | 2 |
| unclassified | `matches` | 0.1.10 | crates.io | MIT | 5 |
| unclassified | `matchit` | 0.7.3 | crates.io | MIT AND BSD-3-Clause | 2 |
| unclassified | `matrixmultiply` | 0.3.10 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `maybe-uninit` | 2.0.0 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `memchr` | 2.7.5 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `memfd` | 0.6.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `memmap2` | 0.9.8 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `memoffset` | 0.5.6 | crates.io | MIT | 4 |
| unclassified | `memoffset` | 0.9.1 | crates.io | MIT | 2 |
| unclassified | `metrics` | 0.21.1 | crates.io | MIT | 2 |
| unclassified | `metrics-macros` | 0.7.1 | crates.io | MIT | 3 |
| unclassified | `mime` | 0.3.17 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `minimal-lexical` | 0.2.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `miniz_oxide` | 0.8.9 | crates.io | MIT OR Zlib OR Apache-2.0 | 2 |
| unclassified | `mio` | 0.8.11 | crates.io | MIT | 2 |
| unclassified | `mio` | 1.0.4 | crates.io | MIT | 2 |
| unclassified | `multiaddr` | 0.18.2 | crates.io | MIT | 2 |
| unclassified | `multibase` | 0.9.1 | crates.io | MIT | 3 |
| unclassified | `multihash` | 0.19.3 | crates.io | MIT | 3 |
| unclassified | `multistream-select` | 0.13.0 | crates.io | MIT | 3 |
| unclassified | `nalgebra` | 0.29.0 | crates.io | BSD-3-Clause | 2 |
| unclassified | `nalgebra` | 0.32.6 | crates.io | BSD-3-Clause | 1 |
| unclassified | `nalgebra-macros` | 0.1.0 | crates.io | Apache-2.0 | 3 |
| unclassified | `nalgebra-macros` | 0.2.2 | crates.io | Apache-2.0 | 2 |
| unclassified | `native-tls` | 0.2.14 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `netlink-packet-core` | 0.7.0 | crates.io | MIT | 4 |
| unclassified | `netlink-packet-route` | 0.17.1 | crates.io | MIT | 4 |
| unclassified | `netlink-packet-utils` | 0.5.2 | crates.io | MIT | 5 |
| unclassified | `netlink-proto` | 0.11.5 | crates.io | MIT | 4 |
| unclassified | `netlink-sys` | 0.8.7 | crates.io | MIT | 4 |
| unclassified | `new_debug_unreachable` | 1.0.6 | crates.io | MIT | 6 |
| unclassified | `nix` | 0.26.4 | crates.io | MIT | 2 |
| unclassified | `nix` | 0.27.1 | crates.io | MIT | 1 |
| unclassified | `nohash-hasher` | 0.2.0 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `nom` | 7.1.3 | crates.io | MIT | 2 |
| unclassified | `normalize-line-endings` | 0.3.0 | crates.io | Apache-2.0 | 2 |
| unclassified | `notify` | 6.1.1 | crates.io | CC0-1.0 | 1 |
| unclassified | `nu-ansi-term` | 0.50.1 | crates.io | MIT | 2 |
| unclassified | `num-bigint` | 0.2.6 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `num-bigint` | 0.4.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `num-complex` | 0.4.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-conv` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-format` | 0.4.4 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `num-integer` | 0.1.46 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `num-rational` | 0.4.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-traits` | 0.2.19 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num_cpus` | 1.17.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `object` | 0.36.7 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `oid-registry` | 0.6.1 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `oid-registry` | 0.7.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `once_cell` | 1.21.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `once_cell_polyfill` | 1.70.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `oorandom` | 11.1.5 | crates.io | MIT | 2 |
| unclassified | `opaque-debug` | 0.3.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `openssl` | 0.10.73 | crates.io | Apache-2.0 | 3 |
| unclassified | `openssl-macros` | 0.1.1 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `openssl-probe` | 0.1.6 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `openssl-src` | 300.5.2+3.5.2 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `openssl-sys` | 0.9.109 | crates.io | MIT | 2 |
| unclassified | `option-ext` | 0.2.0 | crates.io | MPL-2.0 | 3 |
| unclassified | `pairing` | 0.16.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `pairing_ce` | 0.28.6 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `parking` | 2.2.1 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `parking_lot` | 0.11.2 | crates.io | Apache-2.0/MIT | 2 |
| unclassified | `parking_lot` | 0.12.4 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `parking_lot_core` | 0.8.6 | crates.io | Apache-2.0/MIT | 3 |
| unclassified | `parking_lot_core` | 0.9.11 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `paste` | 1.0.15 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `peeking_take_while` | 0.1.2 | crates.io | Apache-2.0/MIT | 4 |
| unclassified | `pem` | 1.1.1 | crates.io | MIT | 5 |
| unclassified | `pem` | 3.0.5 | crates.io | MIT | 3 |
| unclassified | `percent-encoding` | 2.3.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `petgraph` | 0.6.5 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `phf_shared` | 0.11.3 | crates.io | MIT | 6 |
| unclassified | `pico-args` | 0.5.0 | crates.io | MIT | 5 |
| unclassified | `pin-project` | 1.1.10 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `pin-project-internal` | 1.1.10 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `pin-project-lite` | 0.2.16 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `pin-utils` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `piper` | 0.2.4 | crates.io | MIT OR Apache-2.0 | 6 |
| unclassified | `pkcs8` | 0.10.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `pkg-config` | 0.3.32 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `plotters` | 0.3.7 | crates.io | MIT | 2 |
| unclassified | `plotters-backend` | 0.3.7 | crates.io | MIT | 3 |
| unclassified | `plotters-svg` | 0.3.7 | crates.io | MIT | 3 |
| unclassified | `polling` | 3.11.0 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `poly1305` | 0.8.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `polyval` | 0.6.2 | crates.io | Apache-2.0 OR MIT | 6 |
| unclassified | `portable-atomic` | 1.11.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `portable-atomic-util` | 0.2.4 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `postcard` | 1.1.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `potential_utf` | 0.1.3 | crates.io | Unicode-3.0 | 5 |
| unclassified | `powerfmt` | 0.2.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pprof` | 0.13.0 | crates.io | Apache-2.0 | 1 |
| unclassified | `ppv-lite86` | 0.2.21 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `pqcrypto-dilithium` | 0.5.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pqcrypto-internals` | 0.2.11 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `pqcrypto-traits` | 0.3.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `precomputed-hash` | 0.1.1 | crates.io | MIT | 6 |
| unclassified | `predicates` | 3.1.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `predicates-core` | 1.0.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `predicates-tree` | 1.0.12 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `prettyplease` | 0.2.37 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `primal-check` | 0.3.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `probe` | 0.1.0 | workspace | — | 0 |
| unclassified | `proc-macro2` | 1.0.101 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `procfs` | 0.15.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `prometheus` | 0.13.4 | crates.io | Apache-2.0 | 1 |
| unclassified | `prometheus-client` | 0.21.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `prometheus-client-derive-encode` | 0.4.2 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `proptest` | 1.7.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `protobuf` | 2.28.0 | crates.io | MIT | 2 |
| unclassified | `psm` | 0.1.26 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `pyo3-build-config` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3-ffi` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3-macros` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3-macros-backend` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `quick-error` | 1.2.3 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `quick-protobuf` | 0.8.1 | crates.io | MIT | 3 |
| unclassified | `quick-protobuf-codec` | 0.2.0 | crates.io | MIT | 3 |
| unclassified | `quick-xml` | 0.26.0 | crates.io | MIT | 3 |
| unclassified | `quinn-proto` | 0.10.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `quinn-udp` | 0.4.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `quote` | 1.0.40 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `r-efi` | 5.3.0 | crates.io | MIT OR Apache-2.0 OR LGPL-2.1-or-later | 3 |
| unclassified | `rand` | 0.4.6 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `rand` | 0.7.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rand` | 0.8.5 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `rand` | 0.9.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rand_chacha` | 0.2.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `rand_chacha` | 0.3.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rand_chacha` | 0.9.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rand_core` | 0.3.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `rand_core` | 0.4.2 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `rand_core` | 0.5.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `rand_core` | 0.6.4 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `rand_core` | 0.9.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `rand_distr` | 0.4.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `rand_hc` | 0.2.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `rand_xorshift` | 0.2.0 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `rand_xorshift` | 0.4.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rawpointer` | 0.2.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `rayon` | 1.11.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `rayon-core` | 1.13.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rcgen` | 0.10.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `rcgen` | 0.11.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rdrand` | 0.4.0 | crates.io | ISC | 3 |
| unclassified | `redox_syscall` | 0.2.16 | crates.io | MIT | 4 |
| unclassified | `redox_syscall` | 0.5.17 | crates.io | MIT | 3 |
| unclassified | `redox_users` | 0.4.6 | crates.io | MIT | 3 |
| unclassified | `reed-solomon-erasure` | 6.0.0 | crates.io | MIT | 1 |
> **Update (2025-09-30):** `raptorq` and `reed-solomon-erasure` were replaced by in-house coders; the next automated export will drop them from this inventory.
| unclassified | `regalloc2` | 0.9.3 | crates.io | Apache-2.0 WITH LLVM-exception | 4 |
| unclassified | `regex` | 1.11.2 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `regex-automata` | 0.4.10 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `regex-syntax` | 0.8.6 | crates.io | MIT OR Apache-2.0 | 2 |
> **Update (2025-11-05):** `reqwest` has been removed from the workspace in
> favour of the in-house `httpd` crate. The table below still lists the old
> versions from the previous export and will be pruned during the next automated
> refresh.
| unclassified | `reqwest` | 0.11.27 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `reqwest` | 0.12.23 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `resolv-conf` | 0.7.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rgb` | 0.8.52 | crates.io | MIT | 3 |
| unclassified | `ring` | 0.16.20 | crates.io | file:LICENSE | 3 |
| unclassified | `ring` | 0.17.14 | crates.io | Apache-2.0 AND ISC | 2 |
| unclassified | `ripemd` | 0.1.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `rtnetlink` | 0.13.1 | crates.io | MIT | 4 |
| unclassified | `rusqlite` | 0.30.0 | crates.io | MIT | 1 |
| unclassified | `rustc-demangle` | 0.1.26 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `rustc-hash` | 1.1.0 | crates.io | Apache-2.0/MIT | 4 |
| unclassified | `rustc-hash` | 2.1.1 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `rustc_version` | 0.4.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `rustdct` | 0.7.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `rustfft` | 6.4.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rusticata-macros` | 4.1.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `rustix` | 0.36.17 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustix` | 0.37.28 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustix` | 0.38.44 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustix` | 1.1.2 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustls-native-certs` | 0.7.3 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| unclassified | `rustls-pemfile` | 1.0.4 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| unclassified | `rustls-pemfile` | 2.2.0 | crates.io | Apache-2.0 OR ISC OR MIT | 3 |
| unclassified | `rustls-pki-types` | 1.12.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rustls-webpki` | 0.101.7 | crates.io | ISC | 2 |
| unclassified | `rustls-webpki` | 0.102.8 | crates.io | ISC | 3 |
| unclassified | `rustls-webpki` | 0.103.6 | crates.io | ISC | 3 |
| unclassified | `rustversion` | 1.0.22 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rusty-fork` | 0.3.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `rw-stream-sink` | 0.4.0 | crates.io | MIT | 2 |
| unclassified | `ryu` | 1.0.20 | crates.io | Apache-2.0 OR BSL-1.0 | 2 |
| unclassified | `safe_arch` | 0.7.4 | crates.io | Zlib OR Apache-2.0 OR MIT | 4 |
| unclassified | `same-file` | 1.0.6 | crates.io | Unlicense/MIT | 3 |
| unclassified | `scc` | 2.4.0 | crates.io | Apache-2.0 | 2 |
| unclassified | `schannel` | 0.1.28 | crates.io | MIT | 3 |
| unclassified | `scopeguard` | 1.2.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `sct` | 0.7.1 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| unclassified | `sdd` | 3.0.10 | crates.io | Apache-2.0 | 3 |
| unclassified | `security-framework` | 2.11.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `security-framework-sys` | 2.15.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `semver` | 1.0.27 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_bytes` | 0.11.19 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `serde_cbor` | 0.11.2 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `serde_core` | 1.0.224 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_derive` | 1.0.224 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_json` | 1.0.145 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `serde_path_to_error` | 0.1.20 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_regex` | 1.1.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `serde_spanned` | 0.6.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_urlencoded` | 0.7.1 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `serde_yaml` | 0.9.34+deprecated | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serial_test` | 1.0.0 | crates.io | MIT | 2 |
| unclassified | `serial_test` | 3.2.0 | crates.io | MIT | 1 |
| unclassified | `serial_test_derive` | 1.0.0 | crates.io | MIT | 3 |
| unclassified | `serial_test_derive` | 3.2.0 | crates.io | MIT | 2 |
| unclassified | `sha1` | 0.10.6 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `sha2` | 0.10.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `sharded-slab` | 0.1.7 | crates.io | MIT | 2 |
| unclassified | `shlex` | 1.3.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `signal-hook` | 0.3.18 | crates.io | Apache-2.0/MIT | 1 |
| unclassified | `signal-hook-registry` | 1.4.6 | crates.io | Apache-2.0/MIT | 2 |
| unclassified | `signature` | 2.2.0 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `simba` | 0.6.0 | crates.io | Apache-2.0 | 3 |
| unclassified | `simba` | 0.8.1 | crates.io | Apache-2.0 | 2 |
| unclassified | `similar` | 2.7.0 | crates.io | Apache-2.0 | 2 |
| unclassified | `siphasher` | 1.0.1 | crates.io | MIT/Apache-2.0 | 7 |
| unclassified | `slab` | 0.4.11 | crates.io | MIT | 2 |
| unclassified | `slice-group-by` | 0.3.1 | crates.io | MIT | 5 |
| unclassified | `smallvec` | 1.15.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `snow` | 0.9.6 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `socket2` | 0.4.10 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `socket2` | 0.5.10 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `socket2` | 0.6.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `spin` | 0.5.2 | crates.io | MIT | 4 |
| unclassified | `spin` | 0.9.8 | crates.io | MIT | 2 |
| unclassified | `spki` | 0.7.3 | crates.io | Apache-2.0 OR MIT | 4 |
| unclassified | `sptr` | 0.3.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `stable_deref_trait` | 1.2.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `state` | 0.1.0 | workspace | — | 0 |
| unclassified | `static_assertions` | 1.1.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `statrs` | 0.16.1 | crates.io | MIT | 1 |
| unclassified | `storage` | 0.1.0 | workspace | — | 1 |
| unclassified | `str_stack` | 0.1.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `strength_reduce` | 0.2.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `string_cache` | 0.8.9 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `strsim` | 0.11.1 | crates.io | MIT | 3 |
| unclassified | `subtle` | 2.6.1 | crates.io | BSD-3-Clause | 1 |
| unclassified | `symbolic-common` | 12.16.2 | crates.io | MIT | 3 |
| unclassified | `symbolic-demangle` | 12.16.2 | crates.io | MIT | 2 |
| unclassified | `syn` | 1.0.109 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `syn` | 2.0.106 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `sync_wrapper` | 0.1.2 | crates.io | Apache-2.0 | 2 |
| unclassified | `sync_wrapper` | 1.0.2 | crates.io | Apache-2.0 | 2 |
| unclassified | `synstructure` | 0.12.6 | crates.io | MIT | 5 |
| unclassified | `synstructure` | 0.13.2 | crates.io | MIT | 4 |
| unclassified | `system-configuration` | 0.5.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `system-configuration` | 0.6.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `system-configuration-sys` | 0.5.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `system-configuration-sys` | 0.6.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `tar` | 0.4.44 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `target-lexicon` | 0.12.16 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `target-lexicon` | 0.13.3 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `tb-sim` | 0.1.0 | workspace | — | 1 |
| unclassified | `tempfile` | 3.22.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `term` | 0.7.0 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `termcolor` | 1.4.1 | crates.io | Unlicense OR MIT | 4 |
| unclassified | `terminal_size` | 0.2.6 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `termtree` | 0.5.1 | crates.io | MIT | 3 |
| unclassified | `the_block` | 0.1.0 | workspace | — | 0 |
| unclassified | `thiserror` | 1.0.69 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `thiserror` | 2.0.16 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `thiserror-impl` | 1.0.69 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `thiserror-impl` | 2.0.16 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `thread_local` | 1.1.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `threshold_crypto` | 0.4.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `time` | 0.3.43 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `time-core` | 0.1.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `time-macros` | 0.2.24 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `tiny-keccak` | 1.5.0 | crates.io | CC0-1.0 | 2 |
| unclassified | `tiny-keccak` | 2.0.2 | crates.io | CC0-1.0 | 3 |
| unclassified | `tiny_http` | 0.12.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `tinystr` | 0.8.1 | crates.io | Unicode-3.0 | 6 |
| unclassified | `tinytemplate` | 1.2.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `tinyvec` | 1.10.0 | crates.io | Zlib OR Apache-2.0 OR MIT | 2 |
| unclassified | `tinyvec_macros` | 0.1.1 | crates.io | MIT OR Apache-2.0 OR Zlib | 3 |
| unclassified | `tokio-macros` | 2.5.0 | crates.io | MIT | 2 |
| unclassified | `tokio-native-tls` | 0.3.1 | crates.io | MIT | 2 |
| unclassified | `tokio-rustls` | 0.24.1 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `tokio-rustls` | 0.26.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `tokio-util` | 0.7.16 | crates.io | MIT | 1 |
| unclassified | `toml` | 0.8.23 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `toml_datetime` | 0.6.11 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `toml_edit` | 0.22.27 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `toml_write` | 0.1.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `tower` | 0.5.2 | crates.io | MIT | 2 |
| unclassified | `tower-http` | 0.6.6 | crates.io | MIT | 2 |
| unclassified | `tower-layer` | 0.3.3 | crates.io | MIT | 2 |
| unclassified | `tower-service` | 0.3.3 | crates.io | MIT | 2 |
| unclassified | `tracing` | 0.1.41 | crates.io | MIT | 1 |
| unclassified | `tracing-attributes` | 0.1.30 | crates.io | MIT | 2 |
| unclassified | `tracing-chrome` | 0.6.0 | crates.io | MIT | 1 |
| unclassified | `tracing-core` | 0.1.34 | crates.io | MIT | 2 |
| unclassified | `tracing-log` | 0.2.0 | crates.io | MIT | 2 |
| unclassified | `tracing-serde` | 0.2.0 | crates.io | MIT | 2 |
| unclassified | `tracing-subscriber` | 0.3.20 | crates.io | MIT | 1 |
| unclassified | `tracing-test` | 0.2.5 | crates.io | MIT | 1 |
| unclassified | `tracing-test-macro` | 0.2.5 | crates.io | MIT | 2 |
| unclassified | `transpose` | 0.2.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `trust-dns-proto` | 0.22.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `trust-dns-proto` | 0.23.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `trust-dns-resolver` | 0.23.2 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `try-lock` | 0.2.5 | crates.io | MIT | 4 |
| unclassified | `typenum` | 1.18.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `uint` | 0.9.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `unarray` | 0.1.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `unicode-bidi` | 0.3.18 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `unicode-ident` | 1.0.19 | crates.io | (MIT OR Apache-2.0) AND Unicode-3.0 | 4 |
| unclassified | `unicode-normalization` | 0.1.24 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `unicode-width` | 0.2.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `unicode-xid` | 0.2.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `unindent` | 0.2.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `universal-hash` | 0.5.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `unsafe-libyaml` | 0.2.11 | crates.io | MIT | 3 |
| unclassified | `unsigned-varint` | 0.7.2 | crates.io | MIT | 3 |
| unclassified | `unsigned-varint` | 0.8.0 | crates.io | MIT | 3 |
| unclassified | `untrusted` | 0.7.1 | crates.io | ISC | 4 |
| unclassified | `untrusted` | 0.9.0 | crates.io | ISC | 3 |
| unclassified | `ureq` | 2.12.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `url` | 2.5.7 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `utf-8` | 0.7.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `utf8_iter` | 1.0.4 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `utf8parse` | 0.2.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `uuid` | 1.18.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `valuable` | 0.1.1 | crates.io | MIT | 3 |
| unclassified | `value-bag` | 1.11.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `vcpkg` | 0.2.15 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `version_check` | 0.9.5 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `void` | 1.0.2 | crates.io | MIT | 3 |
| unclassified | `wait-timeout` | 0.2.1 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `walkdir` | 2.5.0 | crates.io | Unlicense/MIT | 2 |
| unclassified | `wallet` | 0.1.0 | workspace | — | 1 |
| unclassified | `want` | 0.3.1 | crates.io | MIT | 3 |
| unclassified | `wasi` | 0.11.1+wasi-snapshot-preview1 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `wasi` | 0.14.6+wasi-0.2.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `wasi` | 0.9.0+wasi-snapshot-preview1 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 4 |
| unclassified | `wasip2` | 1.0.1+wasi-0.2.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 4 |
| unclassified | `wasm-bindgen` | 0.2.101 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `wasm-bindgen-backend` | 0.2.101 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `wasm-bindgen-futures` | 0.4.51 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `wasm-bindgen-macro` | 0.2.101 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `wasm-bindgen-macro-support` | 0.2.101 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `wasm-bindgen-shared` | 0.2.101 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `wasm-encoder` | 0.215.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `wasm-encoder` | 0.239.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `wasmparser` | 0.121.2 | crates.io | Apache-2.0 WITH LLVM-exception | 4 |
| unclassified | `wasmparser` | 0.215.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `wasmparser` | 0.239.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 4 |
| unclassified | `wasmprinter` | 0.2.80 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `wasmprinter` | 0.215.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `wasmtime` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 1 |
| unclassified | `wasmtime-asm-macros` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-cache` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-component-macro` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-component-util` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-cranelift` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-environ` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-fiber` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-jit-debug` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-jit-icache-coherence` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-slab` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-types` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `wasmtime-versioned-export-macros` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-winch` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `wasmtime-wit-bindgen` | 24.0.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `wast` | 239.0.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `wat` | 1.239.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 1 |
| unclassified | `web-sys` | 0.3.78 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `webpki-roots` | 0.25.4 | crates.io | MPL-2.0 | 2 |
| unclassified | `webpki-roots` | 0.26.11 | crates.io | CDLA-Permissive-2.0 | 2 |
| unclassified | `webpki-roots` | 1.0.2 | crates.io | CDLA-Permissive-2.0 | 3 |
| unclassified | `wide` | 0.7.33 | crates.io | Zlib OR Apache-2.0 OR MIT | 3 |
| unclassified | `widestring` | 1.2.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `winapi` | 0.3.9 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `winapi-i686-pc-windows-gnu` | 0.4.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `winapi-util` | 0.1.11 | crates.io | Unlicense OR MIT | 3 |
| unclassified | `winapi-x86_64-pc-windows-gnu` | 0.4.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `winch-codegen` | 0.22.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `windows` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-core` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-core` | 0.62.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-implement` | 0.60.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-interface` | 0.59.1 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-link` | 0.1.3 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-link` | 0.2.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-registry` | 0.5.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-result` | 0.1.2 | crates.io | MIT OR Apache-2.0 | 6 |
| unclassified | `windows-result` | 0.3.4 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-result` | 0.4.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-strings` | 0.4.2 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-strings` | 0.5.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-sys` | 0.45.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-sys` | 0.48.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-sys` | 0.52.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-sys` | 0.59.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-sys` | 0.60.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-sys` | 0.61.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-targets` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-targets` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-targets` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-targets` | 0.53.3 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_aarch64_gnullvm` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_aarch64_gnullvm` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_aarch64_gnullvm` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_aarch64_gnullvm` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_aarch64_msvc` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_aarch64_msvc` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_aarch64_msvc` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_aarch64_msvc` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_gnu` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_gnu` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_i686_gnu` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_i686_gnu` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_gnullvm` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_i686_gnullvm` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_msvc` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_i686_msvc` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_i686_msvc` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_i686_msvc` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnu` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnu` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_x86_64_gnu` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_x86_64_gnu` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnullvm` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_gnullvm` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_x86_64_gnullvm` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_x86_64_gnullvm` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_msvc` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows_x86_64_msvc` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_x86_64_msvc` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows_x86_64_msvc` | 0.53.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `winnow` | 0.7.13 | crates.io | MIT | 3 |
| unclassified | `winreg` | 0.50.0 | crates.io | MIT | 2 |
| unclassified | `wit-bindgen` | 0.46.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 5 |
| unclassified | `wit-parser` | 0.215.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `writeable` | 0.6.1 | crates.io | Unicode-3.0 | 6 |
| unclassified | `x25519-dalek` | 2.0.1 | crates.io | BSD-3-Clause | 3 |
| unclassified | `x509-parser` | 0.15.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `x509-parser` | 0.16.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `xattr` | 1.5.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `xml-rs` | 0.8.27 | crates.io | MIT | 5 |
| unclassified | `xmltree` | 0.10.3 | crates.io | MIT | 4 |
| unclassified | `xorfilter-rs` | 0.5.1 | crates.io | Apache-2.0 | 1 |
| unclassified | `xtask` | 0.1.0 | workspace | — | 0 |
| unclassified | `yamux` | 0.12.1 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `yasna` | 0.5.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `yoke` | 0.8.0 | crates.io | Unicode-3.0 | 6 |
| unclassified | `yoke-derive` | 0.8.0 | crates.io | Unicode-3.0 | 7 |
| unclassified | `zerocopy` | 0.8.27 | crates.io | BSD-2-Clause OR Apache-2.0 OR MIT | 4 |
| unclassified | `zerocopy-derive` | 0.8.27 | crates.io | BSD-2-Clause OR Apache-2.0 OR MIT | 5 |
| unclassified | `zerofrom` | 0.1.6 | crates.io | Unicode-3.0 | 6 |
| unclassified | `zerofrom-derive` | 0.1.6 | crates.io | Unicode-3.0 | 7 |
| unclassified | `zeroize` | 1.8.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `zeroize_derive` | 1.4.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `zerotrie` | 0.2.2 | crates.io | Unicode-3.0 | 5 |
| unclassified | `zerovec` | 0.11.4 | crates.io | Unicode-3.0 | 5 |
| unclassified | `zerovec-derive` | 0.11.1 | crates.io | Unicode-3.0 | 6 |
| unclassified | `zstd-safe` | 6.0.6 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `zstd-safe` | 7.2.4 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `zstd-sys` | 2.0.16+zstd.1.5.7 | crates.io | MIT/Apache-2.0 | 3 |
