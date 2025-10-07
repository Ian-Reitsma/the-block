# Dependency Inventory

| Tier | Crate | Version | Origin | License | Depth |
| --- | --- | --- | --- | --- | --- |
> **2025-10-07 note**: Workspace crates now consume the first-party `base64_fp` encoder/decoder; the remaining `base64` entries
> below are transitive pulls scheduled for replacement once upstream dependencies are rewritten or patched to accept the in-house
> implementation.
| strategic | `rustls` | 0.23.32 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| replaceable | `bincode` | 1.3.3 | crates.io | MIT | 1 |
| replaceable | `serde` | 1.0.228 | crates.io | MIT OR Apache-2.0 | 1 |
| replaceable | `sled` | 0.34.7 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `addr2line` | 0.22.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `addr2line` | 0.25.1 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `adler2` | 2.0.1 | crates.io | 0BSD OR MIT OR Apache-2.0 | 3 |
| unclassified | `ahash` | 0.8.12 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `aho-corasick` | 1.1.3 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `allocator-api2` | 0.2.21 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `android_system_properties` | 0.1.5 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `anes` | 0.1.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `anstream` | 0.6.21 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `anstyle` | 1.0.13 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `anstyle-parse` | 0.2.7 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `anstyle-query` | 1.1.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `anstyle-wincon` | 3.0.10 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `anyhow` | 1.0.100 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `approx` | 0.5.1 | crates.io | Apache-2.0 | 2 |
| unclassified | `arbitrary` | 1.4.2 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `arrayvec` | 0.7.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `asn1-rs` | 0.6.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `asn1-rs-derive` | 0.5.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `asn1-rs-impl` | 0.2.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `assert_cmd` | 2.0.17 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `async-trait` | 0.1.89 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `autocfg` | 1.5.0 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `backtrace` | 0.3.76 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `base64` | 0.21.7 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `base64` | 0.22.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `base64ct` | 1.8.0 | crates.io | Apache-2.0 OR MIT | 1 |
| unclassified | `bindgen` | 0.65.1 | crates.io | BSD-3-Clause | 3 |
| unclassified | `bindgen` | 0.72.1 | crates.io | BSD-3-Clause | 4 |
| unclassified | `bit-set` | 0.8.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `bit-vec` | 0.8.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `bitflags` | 1.3.2 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `bitflags` | 2.9.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `block-buffer` | 0.10.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `bridges` | 0.1.0 | workspace | — | 1 |
| unclassified | `bs58` | 0.4.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `bstr` | 1.12.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `bumpalo` | 3.19.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `bytemuck` | 1.24.0 | crates.io | Zlib OR Apache-2.0 OR MIT | 4 |
| unclassified | `byteorder` | 1.5.0 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `bytes` | 1.10.1 | crates.io | MIT | 1 |
| unclassified | `bzip2-sys` | 0.1.13+1.0.8 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `cast` | 0.3.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `cc` | 1.2.40 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `cexpr` | 0.6.0 | crates.io | Apache-2.0/MIT | 4 |
| unclassified | `cfg-if` | 1.0.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `chrono` | 0.4.42 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ciborium` | 0.2.2 | crates.io | Apache-2.0 | 2 |
| unclassified | `ciborium-io` | 0.2.2 | crates.io | Apache-2.0 | 3 |
| unclassified | `ciborium-ll` | 0.2.2 | crates.io | Apache-2.0 | 3 |
| unclassified | `clang-sys` | 1.8.1 | crates.io | Apache-2.0 | 4 |
| unclassified | `clap` | 4.5.48 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `clap_builder` | 4.5.48 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `clap_lex` | 0.7.5 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `cli_core` | 0.1.0 | workspace | MIT OR Apache-2.0 | 1 |
| unclassified | `cobs` | 0.3.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `codec` | 0.1.0 | workspace | — | 1 |
| unclassified | `coding` | 0.1.0 | workspace | — | 1 |
| unclassified | `colorchoice` | 1.0.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `colored` | 2.2.0 | crates.io | MPL-2.0 | 1 |
| unclassified | `console` | 0.15.11 | crates.io | MIT | 2 |
| unclassified | `core-foundation` | 0.9.4 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `core-foundation-sys` | 0.8.7 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `cpp_demangle` | 0.4.5 | crates.io | MIT OR Apache-2.0 | 3 |
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
| unclassified | `crossbeam` | 0.8.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-channel` | 0.5.15 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-deque` | 0.8.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-epoch` | 0.9.18 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crossbeam-queue` | 0.3.12 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `crossbeam-utils` | 0.8.21 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `crunchy` | 0.2.4 | crates.io | MIT | 4 |
| unclassified | `crypto` | 0.1.0 | workspace | — | 1 |
| unclassified | `crypto-common` | 0.1.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `crypto_suite` | 0.1.0 | workspace | — | 1 |
| unclassified | `csv` | 1.3.1 | crates.io | Unlicense/MIT | 1 |
| unclassified | `csv-core` | 0.1.12 | crates.io | Unlicense/MIT | 2 |
| unclassified | `dashmap` | 5.5.3 | crates.io | MIT | 1 |
| unclassified | `data-encoding` | 2.9.0 | crates.io | MIT | 2 |
| unclassified | `debugid` | 0.8.0 | crates.io | Apache-2.0 | 3 |
| unclassified | `der-parser` | 9.0.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `deranged` | 0.5.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `derive_arbitrary` | 1.4.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `dex` | 0.1.0 | workspace | — | 1 |
| unclassified | `difflib` | 0.4.0 | crates.io | MIT | 2 |
| unclassified | `digest` | 0.10.7 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `directories-next` | 2.0.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `dirs` | 5.0.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `dirs-sys` | 0.4.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `dirs-sys-next` | 0.1.2 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `displaydoc` | 0.2.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `dkg` | 0.1.0 | workspace | — | 1 |
| unclassified | `doc-comment` | 0.3.3 | crates.io | MIT | 2 |
| unclassified | `dunce` | 1.0.5 | crates.io | CC0-1.0 OR MIT-0 OR Apache-2.0 | 4 |
| unclassified | `either` | 1.15.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `embedded-io` | 0.4.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `embedded-io` | 0.6.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `encode_unicode` | 1.0.0 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `encoding_rs` | 0.8.35 | crates.io | (Apache-2.0 OR MIT) AND BSD-3-Clause | 2 |
| unclassified | `env_filter` | 0.1.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `env_logger` | 0.11.8 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `equivalent` | 1.0.2 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `errno` | 0.3.14 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `explorer` | 0.1.0 | workspace | — | 2 |
| unclassified | `failure` | 0.1.8 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `failure_derive` | 0.1.8 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `fallible-iterator` | 0.3.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `fallible-streaming-iterator` | 0.1.9 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `fastrand` | 2.3.0 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `ff` | 0.6.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `ff_derive` | 0.6.0 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `filetime` | 0.2.26 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `find-msvc-tools` | 0.1.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `findshlibs` | 0.10.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `flate2` | 1.1.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `float-cmp` | 0.10.0 | crates.io | MIT | 2 |
| unclassified | `fnv` | 1.0.7 | crates.io | Apache-2.0 / MIT | 2 |
| unclassified | `foldhash` | 0.1.5 | crates.io | Zlib | 4 |
| unclassified | `foreign-types` | 0.3.2 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `foreign-types-shared` | 0.1.1 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `form_urlencoded` | 1.2.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `fs2` | 0.4.3 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `fuchsia-cprng` | 0.1.1 | crates.io | file:LICENSE | 3 |
| unclassified | `futures` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `futures-channel` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-core` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-executor` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-io` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-macro` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `futures-sink` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-task` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `futures-util` | 0.3.31 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `fxhash` | 0.2.1 | crates.io | Apache-2.0/MIT | 2 |
| unclassified | `fxprof-processed-profile` | 0.6.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `generic-array` | 0.14.7 | crates.io | MIT | 4 |
| unclassified | `getrandom` | 0.1.16 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `getrandom` | 0.2.16 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `getrandom` | 0.3.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `gimli` | 0.29.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `gimli` | 0.32.3 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `git2` | 0.18.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `glob` | 0.3.3 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `globset` | 0.4.16 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `governance` | 0.1.0 | workspace | — | 1 |
| unclassified | `group` | 0.6.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `half` | 1.8.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `half` | 2.6.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `hashbrown` | 0.13.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `hashbrown` | 0.14.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hashbrown` | 0.15.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `hashbrown` | 0.16.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hashlink` | 0.8.4 | crates.io | MIT OR Apache-2.0 | 2 |
| workspace | `histogram_fp` | 0.1.0 | workspace | — | 1 |
| unclassified | `heck` | 0.4.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `heck` | 0.5.0 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `hermit-abi` | 0.3.9 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `hermit-abi` | 0.5.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `hex` | 0.4.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `hex_fmt` | 0.3.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `hidapi` | 2.6.3 | crates.io | MIT | 2 |
| unclassified | `httparse` | 1.10.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `httpd` | 0.1.0 | workspace | — | 1 |
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
| unclassified | `idna` | 1.1.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `idna_adapter` | 1.2.1 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `indexmap` | 2.11.4 | crates.io | Apache-2.0 OR MIT | 1 |
| unclassified | `indoc` | 2.0.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `inferno` | 0.11.21 | crates.io | CDDL-1.0 | 2 |
| unclassified | `inflation` | 0.1.0 | workspace | — | 1 |
| unclassified | `insta` | 1.43.2 | crates.io | Apache-2.0 | 1 |
| unclassified | `instant` | 0.1.13 | crates.io | BSD-3-Clause | 3 |
| unclassified | `io-lifetimes` | 1.0.11 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `is-terminal` | 0.4.16 | crates.io | MIT | 2 |
| unclassified | `is_terminal_polyfill` | 1.70.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `itertools` | 0.10.5 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `itertools` | 0.12.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `itertools` | 0.13.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `itoa` | 1.0.15 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `ittapi` | 0.4.0 | crates.io | GPL-2.0-only OR BSD-3-Clause | 2 |
| unclassified | `ittapi-sys` | 0.4.0 | crates.io | GPL-2.0-only OR BSD-3-Clause | 3 |
| unclassified | `jiff` | 0.2.15 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `jiff-static` | 0.2.15 | crates.io | Unlicense OR MIT | 3 |
| unclassified | `jobserver` | 0.1.34 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `js-sys` | 0.3.81 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `json` | 0.12.4 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `jsonrpc-core` | 18.0.0 | crates.io | MIT | 1 |
| unclassified | `jurisdiction` | 0.1.0 | workspace | — | 1 |
| unclassified | `lazy_static` | 1.5.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `lazycell` | 1.3.0 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `leb128` | 0.2.5 | crates.io | Apache-2.0/MIT | 3 |
| unclassified | `leb128fmt` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `ledger` | 0.1.0 | workspace | — | 1 |
| unclassified | `libc` | 0.2.176 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `libgit2-sys` | 0.16.2+1.7.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `libloading` | 0.8.9 | crates.io | ISC | 5 |
| unclassified | `libm` | 0.2.15 | crates.io | MIT | 2 |
| unclassified | `libredox` | 0.1.10 | crates.io | MIT | 3 |
| unclassified | `librocksdb-sys` | 0.11.0+8.1.1 | crates.io | MIT/Apache-2.0/BSD-3-Clause | 2 |
| unclassified | `libsqlite3-sys` | 0.27.0 | crates.io | MIT | 2 |
| unclassified | `libssh2-sys` | 0.3.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `libz-sys` | 1.1.22 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `light-client` | 0.1.0 | workspace | — | 1 |
| unclassified | `linux-raw-sys` | 0.1.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `linux-raw-sys` | 0.11.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `linux-raw-sys` | 0.3.8 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `linux-raw-sys` | 0.4.15 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `litemap` | 0.8.0 | crates.io | Unicode-3.0 | 6 |
| unclassified | `lock_api` | 0.4.14 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `log` | 0.4.28 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `logtest` | 2.0.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `lru` | 0.11.1 | crates.io | MIT | 1 |
| unclassified | `lz4-sys` | 1.11.1+lz4-1.10.0 | crates.io | MIT | 3 |
| unclassified | `mach2` | 0.4.3 | crates.io | BSD-2-Clause OR MIT OR Apache-2.0 | 2 |
| unclassified | `matchers` | 0.2.0 | crates.io | MIT | 2 |
| unclassified | `matrixmultiply` | 0.3.10 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `memchr` | 2.7.6 | crates.io | Unlicense OR MIT | 2 |
| unclassified | `memfd` | 0.6.5 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `memmap2` | 0.9.8 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `memoffset` | 0.9.1 | crates.io | MIT | 2 |
| unclassified | `metrics` | 0.21.1 | crates.io | MIT | 2 |
| unclassified | `metrics-macros` | 0.7.1 | crates.io | MIT | 3 |
| unclassified | `minimal-lexical` | 0.2.1 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `miniz_oxide` | 0.8.9 | crates.io | MIT OR Zlib OR Apache-2.0 | 2 |
| unclassified | `mio` | 0.8.11 | crates.io | MIT | 2 |
| unclassified | `nalgebra` | 0.29.0 | crates.io | BSD-3-Clause | 2 |
| unclassified | `nalgebra` | 0.32.6 | crates.io | BSD-3-Clause | 1 |
| unclassified | `nalgebra-macros` | 0.1.0 | crates.io | Apache-2.0 | 3 |
| unclassified | `nalgebra-macros` | 0.2.2 | crates.io | Apache-2.0 | 2 |
| unclassified | `native-tls` | 0.2.14 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `nix` | 0.26.4 | crates.io | MIT | 2 |
| unclassified | `nix` | 0.27.1 | crates.io | MIT | 1 |
| unclassified | `nom` | 7.1.3 | crates.io | MIT | 2 |
| unclassified | `normalize-line-endings` | 0.3.0 | crates.io | Apache-2.0 | 2 |
| unclassified | `nu-ansi-term` | 0.50.1 | crates.io | MIT | 2 |
| unclassified | `num-bigint` | 0.2.6 | crates.io | MIT/Apache-2.0 | 5 |
| unclassified | `num-bigint` | 0.4.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-complex` | 0.4.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-conv` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-format` | 0.4.4 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `num-integer` | 0.1.46 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `num-rational` | 0.4.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num-traits` | 0.2.19 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `num_cpus` | 1.17.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `object` | 0.36.7 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `object` | 0.37.3 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `oid-registry` | 0.7.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `once_cell` | 1.21.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `once_cell_polyfill` | 1.70.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `oorandom` | 11.1.5 | crates.io | MIT | 2 |
| unclassified | `openssl` | 0.10.73 | crates.io | Apache-2.0 | 3 |
| unclassified | `openssl-macros` | 0.1.1 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `openssl-probe` | 0.1.6 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `openssl-sys` | 0.9.109 | crates.io | MIT | 2 |
| unclassified | `option-ext` | 0.2.0 | crates.io | MPL-2.0 | 3 |
| unclassified | `p2p_overlay` | 0.1.0 | workspace | — | 1 |
| unclassified | `pairing` | 0.16.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `parking_lot` | 0.11.2 | crates.io | Apache-2.0/MIT | 2 |
| unclassified | `parking_lot` | 0.12.5 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `parking_lot_core` | 0.8.6 | crates.io | Apache-2.0/MIT | 3 |
| unclassified | `parking_lot_core` | 0.9.12 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `paste` | 1.0.15 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `peeking_take_while` | 0.1.2 | crates.io | Apache-2.0/MIT | 4 |
| unclassified | `pem` | 3.0.5 | crates.io | MIT | 3 |
| unclassified | `percent-encoding` | 2.3.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pin-project` | 1.1.10 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `pin-project-internal` | 1.1.10 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `pin-project-lite` | 0.2.16 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `pin-utils` | 0.1.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `pkg-config` | 0.3.32 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `plotters` | 0.3.7 | crates.io | MIT | 2 |
| unclassified | `plotters-backend` | 0.3.7 | crates.io | MIT | 3 |
| unclassified | `plotters-svg` | 0.3.7 | crates.io | MIT | 3 |
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
| unclassified | `predicates` | 3.1.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `predicates-core` | 1.0.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `predicates-tree` | 1.0.12 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `prettyplease` | 0.2.37 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `primal-check` | 0.3.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `probe` | 0.1.0 | workspace | — | 0 |
| unclassified | `proc-macro2` | 1.0.101 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `procfs` | 0.15.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `prometheus` | 0.13.4 | crates.io | Apache-2.0 | 1 |
| unclassified | `proptest` | 1.8.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `protobuf` | 2.28.0 | crates.io | MIT | 2 |
| unclassified | `psm` | 0.1.27 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `pyo3-build-config` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3-ffi` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3-macros` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `pyo3-macros-backend` | 0.24.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `quick-error` | 1.2.3 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `quick-xml` | 0.26.0 | crates.io | MIT | 3 |
| unclassified | `quote` | 1.0.41 | crates.io | MIT OR Apache-2.0 | 3 |
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
| unclassified | `rcgen` | 0.11.3 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rdrand` | 0.4.0 | crates.io | ISC | 3 |
| unclassified | `redox_syscall` | 0.2.16 | crates.io | MIT | 4 |
| unclassified | `redox_syscall` | 0.5.18 | crates.io | MIT | 3 |
| unclassified | `redox_users` | 0.4.6 | crates.io | MIT | 3 |
| unclassified | `regalloc2` | 0.9.3 | crates.io | Apache-2.0 WITH LLVM-exception | 4 |
| unclassified | `regex` | 1.11.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `regex-automata` | 0.4.11 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `regex-syntax` | 0.8.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rgb` | 0.8.52 | crates.io | MIT | 3 |
| unclassified | `ring` | 0.16.20 | crates.io | file:LICENSE | 3 |
| unclassified | `ring` | 0.17.14 | crates.io | Apache-2.0 AND ISC | 3 |
| unclassified | `ripemd` | 0.1.3 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `runtime` | 0.1.0 | workspace | — | 1 |
| unclassified | `rusqlite` | 0.30.0 | crates.io | MIT | 1 |
| unclassified | `rustc-demangle` | 0.1.26 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `rustc-hash` | 1.1.0 | crates.io | Apache-2.0/MIT | 4 |
| unclassified | `rustc-hash` | 2.1.1 | crates.io | Apache-2.0 OR MIT | 5 |
| unclassified | `rustdct` | 0.7.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `rustfft` | 6.4.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rusticata-macros` | 4.1.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `rustix` | 0.36.17 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustix` | 0.37.28 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustix` | 0.38.44 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustix` | 1.1.2 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| unclassified | `rustls-pemfile` | 2.2.0 | crates.io | Apache-2.0 OR ISC OR MIT | 2 |
| unclassified | `rustls-pki-types` | 1.12.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `rustls-webpki` | 0.103.7 | crates.io | ISC | 3 |
| unclassified | `rustversion` | 1.0.22 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `rusty-fork` | 0.3.1 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `ryu` | 1.0.20 | crates.io | Apache-2.0 OR BSL-1.0 | 2 |
| unclassified | `safe_arch` | 0.7.4 | crates.io | Zlib OR Apache-2.0 OR MIT | 4 |
| unclassified | `same-file` | 1.0.6 | crates.io | Unlicense/MIT | 3 |
| unclassified | `scc` | 2.4.0 | crates.io | Apache-2.0 | 2 |
| unclassified | `schannel` | 0.1.28 | crates.io | MIT | 3 |
| unclassified | `scopeguard` | 1.2.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `sdd` | 3.0.10 | crates.io | Apache-2.0 | 3 |
| unclassified | `security-framework` | 2.11.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `security-framework-sys` | 2.15.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `semver` | 1.0.27 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_bytes` | 0.11.19 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `serde_cbor` | 0.11.2 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `serde_core` | 1.0.228 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_derive` | 1.0.228 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_json` | 1.0.145 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `serde_spanned` | 0.6.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serde_yaml` | 0.9.34+deprecated | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `serial_test` | 1.0.0 | crates.io | MIT | 2 |
| unclassified | `serial_test` | 3.2.0 | crates.io | MIT | 1 |
| unclassified | `serial_test_derive` | 1.0.0 | crates.io | MIT | 3 |
| unclassified | `serial_test_derive` | 3.2.0 | crates.io | MIT | 2 |
| unclassified | `sha1` | 0.10.6 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `sha2` | 0.10.9 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `sharded-slab` | 0.1.7 | crates.io | MIT | 2 |
| unclassified | `shlex` | 1.3.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `signal-hook` | 0.3.18 | crates.io | Apache-2.0/MIT | 1 |
| unclassified | `signal-hook-registry` | 1.4.6 | crates.io | Apache-2.0/MIT | 2 |
| unclassified | `simba` | 0.6.0 | crates.io | Apache-2.0 | 3 |
| unclassified | `simba` | 0.8.1 | crates.io | Apache-2.0 | 2 |
| unclassified | `simd-adler32` | 0.3.7 | crates.io | MIT | 3 |
| unclassified | `similar` | 2.7.0 | crates.io | Apache-2.0 | 2 |
| unclassified | `slab` | 0.4.11 | crates.io | MIT | 3 |
| unclassified | `slice-group-by` | 0.3.1 | crates.io | MIT | 5 |
| unclassified | `smallvec` | 1.15.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `socket2` | 0.5.10 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `spin` | 0.5.2 | crates.io | MIT | 4 |
| unclassified | `sptr` | 0.3.2 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `stable_deref_trait` | 1.2.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `state` | 0.1.0 | workspace | — | 0 |
| unclassified | `static_assertions` | 1.1.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `statrs` | 0.16.1 | crates.io | MIT | 1 |
| unclassified | `storage` | 0.1.0 | workspace | — | 1 |
| unclassified | `storage_engine` | 0.1.0 | workspace | — | 1 |
| unclassified | `str_stack` | 0.1.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `strength_reduce` | 0.2.4 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `subtle` | 2.6.1 | crates.io | BSD-3-Clause | 1 |
| unclassified | `symbolic-common` | 12.16.3 | crates.io | MIT | 3 |
| unclassified | `symbolic-demangle` | 12.16.3 | crates.io | MIT | 2 |
| unclassified | `syn` | 1.0.109 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `syn` | 2.0.106 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `synstructure` | 0.12.6 | crates.io | MIT | 5 |
| unclassified | `synstructure` | 0.13.2 | crates.io | MIT | 4 |
| unclassified | `tar` | 0.4.44 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `target-lexicon` | 0.12.16 | crates.io | Apache-2.0 WITH LLVM-exception | 2 |
| unclassified | `target-lexicon` | 0.13.3 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `tb-sim` | 0.1.0 | workspace | — | 1 |
| unclassified | `tempfile` | 3.23.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `termcolor` | 1.4.1 | crates.io | Unlicense OR MIT | 4 |
| unclassified | `terminal_size` | 0.2.6 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `termtree` | 0.5.1 | crates.io | MIT | 3 |
| unclassified | `the_block` | 0.1.0 | workspace | — | 0 |
| unclassified | `thiserror` | 1.0.69 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `thiserror` | 2.0.17 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `thiserror-impl` | 1.0.69 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `thiserror-impl` | 2.0.17 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `thread_local` | 1.1.9 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `threshold_crypto` | 0.4.0 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `time` | 0.3.44 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `time-core` | 0.1.6 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `time-macros` | 0.2.24 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `tiny-keccak` | 2.0.2 | crates.io | CC0-1.0 | 3 |
| unclassified | `tinystr` | 0.8.1 | crates.io | Unicode-3.0 | 6 |
| unclassified | `tinytemplate` | 1.2.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `tinyvec` | 1.10.0 | crates.io | Zlib OR Apache-2.0 OR MIT | 2 |
| unclassified | `tinyvec_macros` | 0.1.1 | crates.io | MIT OR Apache-2.0 OR Zlib | 3 |
| unclassified | `toml` | 0.8.23 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `toml_datetime` | 0.6.11 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `toml_edit` | 0.22.27 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `toml_write` | 0.1.2 | crates.io | MIT OR Apache-2.0 | 3 |
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
| unclassified | `typenum` | 1.19.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `unarray` | 0.1.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `unicode-ident` | 1.0.19 | crates.io | (MIT OR Apache-2.0) AND Unicode-3.0 | 4 |
| unclassified | `unicode-normalization` | 0.1.24 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `unicode-width` | 0.2.1 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `unicode-xid` | 0.2.6 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `unindent` | 0.2.4 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `unsafe-libyaml` | 0.2.11 | crates.io | MIT | 3 |
| unclassified | `untrusted` | 0.7.1 | crates.io | ISC | 4 |
| unclassified | `untrusted` | 0.9.0 | crates.io | ISC | 4 |
| unclassified | `ureq` | 2.12.1 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `url` | 2.5.7 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `utf8_iter` | 1.0.4 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `utf8parse` | 0.2.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `uuid` | 1.18.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `valuable` | 0.1.1 | crates.io | MIT | 3 |
| unclassified | `value-bag` | 1.11.1 | crates.io | Apache-2.0 OR MIT | 2 |
| unclassified | `vcpkg` | 0.2.15 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `version_check` | 0.9.5 | crates.io | MIT/Apache-2.0 | 4 |
| unclassified | `wait-timeout` | 0.2.1 | crates.io | MIT/Apache-2.0 | 1 |
| unclassified | `walkdir` | 2.5.0 | crates.io | Unlicense/MIT | 2 |
| unclassified | `wallet` | 0.1.0 | workspace | — | 1 |
| unclassified | `wasi` | 0.11.1+wasi-snapshot-preview1 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `wasi` | 0.14.7+wasi-0.2.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `wasi` | 0.9.0+wasi-snapshot-preview1 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 4 |
| unclassified | `wasip2` | 1.0.1+wasi-0.2.4 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 4 |
| unclassified | `wasm-bindgen` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `wasm-bindgen-backend` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 6 |
| unclassified | `wasm-bindgen-macro` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `wasm-bindgen-macro-support` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `wasm-bindgen-shared` | 0.2.104 | crates.io | MIT OR Apache-2.0 | 4 |
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
| unclassified | `web-sys` | 0.3.81 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `webpki-roots` | 0.26.11 | crates.io | CDLA-Permissive-2.0 | 2 |
| unclassified | `webpki-roots` | 1.0.2 | crates.io | CDLA-Permissive-2.0 | 3 |
| unclassified | `wide` | 0.7.33 | crates.io | Zlib OR Apache-2.0 OR MIT | 3 |
| unclassified | `winapi` | 0.3.9 | crates.io | MIT/Apache-2.0 | 2 |
| unclassified | `winapi-i686-pc-windows-gnu` | 0.4.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `winapi-util` | 0.1.11 | crates.io | Unlicense OR MIT | 3 |
| unclassified | `winapi-x86_64-pc-windows-gnu` | 0.4.0 | crates.io | MIT/Apache-2.0 | 3 |
| unclassified | `winch-codegen` | 0.22.4 | crates.io | Apache-2.0 WITH LLVM-exception | 3 |
| unclassified | `windows-core` | 0.62.1 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-implement` | 0.60.1 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-interface` | 0.59.2 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-link` | 0.2.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-result` | 0.4.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-strings` | 0.5.0 | crates.io | MIT OR Apache-2.0 | 5 |
| unclassified | `windows-sys` | 0.45.0 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-sys` | 0.48.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-sys` | 0.52.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-sys` | 0.59.0 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-sys` | 0.60.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-sys` | 0.61.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `windows-targets` | 0.42.2 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `windows-targets` | 0.48.5 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-targets` | 0.52.6 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `windows-targets` | 0.53.4 | crates.io | MIT OR Apache-2.0 | 4 |
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
| unclassified | `wit-bindgen` | 0.46.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 5 |
| unclassified | `wit-parser` | 0.215.0 | crates.io | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 3 |
| unclassified | `writeable` | 0.6.1 | crates.io | Unicode-3.0 | 6 |
| unclassified | `x509-parser` | 0.16.0 | crates.io | MIT OR Apache-2.0 | 1 |
| unclassified | `xattr` | 1.6.1 | crates.io | MIT OR Apache-2.0 | 2 |
| unclassified | `xorfilter-rs` | 0.5.1 | crates.io | Apache-2.0 | 1 |
| unclassified | `xtask` | 0.1.0 | workspace | — | 0 |
| unclassified | `yasna` | 0.5.2 | crates.io | MIT OR Apache-2.0 | 3 |
| unclassified | `yoke` | 0.8.0 | crates.io | Unicode-3.0 | 6 |
| unclassified | `yoke-derive` | 0.8.0 | crates.io | Unicode-3.0 | 7 |
| unclassified | `zerocopy` | 0.8.27 | crates.io | BSD-2-Clause OR Apache-2.0 OR MIT | 4 |
| unclassified | `zerocopy-derive` | 0.8.27 | crates.io | BSD-2-Clause OR Apache-2.0 OR MIT | 5 |
| unclassified | `zerofrom` | 0.1.6 | crates.io | Unicode-3.0 | 6 |
| unclassified | `zerofrom-derive` | 0.1.6 | crates.io | Unicode-3.0 | 7 |
| unclassified | `zeroize` | 1.8.2 | crates.io | Apache-2.0 OR MIT | 3 |
| unclassified | `zerotrie` | 0.2.2 | crates.io | Unicode-3.0 | 5 |
| unclassified | `zerovec` | 0.11.4 | crates.io | Unicode-3.0 | 5 |
| unclassified | `zerovec-derive` | 0.11.1 | crates.io | Unicode-3.0 | 6 |
| unclassified | `zstd` | 0.13.3 | crates.io | MIT | 3 |
| unclassified | `zstd-safe` | 7.2.4 | crates.io | MIT OR Apache-2.0 | 4 |
| unclassified | `zstd-sys` | 2.0.16+zstd.1.5.7 | crates.io | MIT/Apache-2.0 | 3 |
