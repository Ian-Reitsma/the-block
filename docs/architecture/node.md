# Node Dependency Tree

This document lists the dependency hierarchy for the `the_block` node crate. It is generated via `cargo tree --manifest-path node/Cargo.toml`.

```
the_block v0.1.0 (/workspace/the-block/node)
├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
├── bridges v0.1.0 (/workspace/the-block/bridges)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite)
│   │   ├── codec v0.1.0 (/workspace/the-block/crates/codec)
│   │   │   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization)
│   │   │   │   ├── serde v1.0.228
│   │   │   │   │   ├── serde_core v1.0.228
│   │   │   │   │   └── serde_derive v1.0.228 (proc-macro)
│   │   │   │   │       ├── proc-macro2 v1.0.101
│   │   │   │   │       │   └── unicode-ident v1.0.19
│   │   │   │   │       ├── quote v1.0.41
│   │   │   │   │       │   └── proc-macro2 v1.0.101 (*)
│   │   │   │   │       └── syn v2.0.106
│   │   │   │   │           ├── proc-macro2 v1.0.101 (*)
│   │   │   │   │           ├── quote v1.0.41 (*)
│   │   │   │   │           └── unicode-ident v1.0.19
│   │   │   │   └── serde_bytes v0.11.19
│   │   │   │       └── serde_core v1.0.228
│   │   │   ├── serde v1.0.228 (*)
│   │   │   └── thiserror v1.0.69
│   │   │       └── thiserror-impl v1.0.69 (proc-macro)
│   │   │           ├── proc-macro2 v1.0.101 (*)
│   │   │           ├── quote v1.0.41 (*)
│   │   │           └── syn v2.0.106 (*)
│   │   ├── foundation_lazy v0.1.0 (/workspace/the-block/crates/foundation_lazy)
│   │   ├── num-bigint v0.4.6
│   │   │   ├── num-integer v0.1.46
│   │   │   │   └── num-traits v0.2.19
│   │   │   │       [build-dependencies]
│   │   │   │       └── autocfg v1.5.0
│   │   │   └── num-traits v0.2.19 (*)
│   │   ├── num-traits v0.2.19 (*)
│   │   ├── rand v0.1.0 (/workspace/the-block/crates/rand)
│   │   │   └── rand_core v0.1.0 (/workspace/the-block/crates/rand_core)
│   │   ├── serde v1.0.228 (*)
│   │   └── thiserror v1.0.69 (*)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── ledger v0.1.0 (/workspace/the-block/ledger)
│   │   ├── cli_core v0.1.0 (/workspace/the-block/crates/cli_core)
│   │   │   └── thiserror v1.0.69 (*)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   │   ├── serde v1.0.228 (*)
│   │   └── storage_engine v0.1.0 (/workspace/the-block/crates/storage_engine)
│   │       ├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
│   │       ├── diagnostics v0.1.0 (/workspace/the-block/crates/diagnostics)
│   │       ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   │       └── sys v0.1.0 (/workspace/the-block/crates/sys)
│   │           ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │           └── libc v0.2.176
│   │       [build-dependencies]
│   │       └── dependency_guard v0.1.0 (/workspace/the-block/crates/dependency_guard)
│   ├── serde v1.0.228 (*)
│   └── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
├── cli_core v0.1.0 (/workspace/the-block/crates/cli_core) (*)
├── codec v0.1.0 (/workspace/the-block/crates/codec) (*)
├── coding v0.1.0 (/workspace/the-block/crates/coding)
│   ├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
│   ├── crypto v0.1.0 (/workspace/the-block/crypto)
│   │   ├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   └── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── serde v1.0.228 (*)
│   ├── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
│   └── thiserror v1.0.69 (*)
├── foundation_tui v0.1.0 (/workspace/the-block/crates/foundation_tui)
│   └── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
├── concurrency v0.1.0 (/workspace/the-block/crates/concurrency)
│   └── serde v1.0.228 (*)
├── crypto v0.1.0 (/workspace/the-block/crypto) (*)
├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
├── dex v0.1.0 (/workspace/the-block/dex)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── serde v1.0.228 (*)
│   └── subtle v2.6.1
├── diagnostics v0.1.0 (/workspace/the-block/crates/diagnostics)
├── dkg v0.1.0 (/workspace/the-block/dkg)
│   └── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
├── foundation_archive v0.1.0 (/workspace/the-block/crates/foundation_archive)
├── foundation_math v0.1.0 (/workspace/the-block/crates/foundation_math)
├── foundation_regex v0.1.0 (/workspace/the-block/crates/foundation_regex)
├── foundation_rpc v0.1.0 (/workspace/the-block/crates/foundation_rpc)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── httpd v0.1.0 (/workspace/the-block/crates/httpd)
│   │   ├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   ├── diagnostics v0.1.0 (/workspace/the-block/crates/diagnostics)
│   │   ├── foundation_regex v0.1.0 (/workspace/the-block/crates/foundation_regex)
│   │   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   │   ├── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
│   │   └── runtime v0.1.0 (/workspace/the-block/crates/runtime)
│   │       ├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
│   │       ├── concurrency v0.1.0 (/workspace/the-block/crates/concurrency) (*)
│   │       ├── crossbeam-deque v0.8.6
│   │       │   ├── crossbeam-epoch v0.9.18
│   │       │   │   └── crossbeam-utils v0.8.21
│   │       │   └── crossbeam-utils v0.8.21
│   │       ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │       ├── futures v0.3.31
│   │       │   ├── futures-channel v0.3.31
│   │       │   │   ├── futures-core v0.3.31
│   │       │   │   └── futures-sink v0.3.31
│   │       │   ├── futures-core v0.3.31
│   │       │   ├── futures-executor v0.3.31
│   │       │   │   ├── futures-core v0.3.31
│   │       │   │   ├── futures-task v0.3.31
│   │       │   │   └── futures-util v0.3.31
│   │       │   │       ├── futures-channel v0.3.31 (*)
│   │       │   │       ├── futures-core v0.3.31
│   │       │   │       ├── futures-io v0.3.31
│   │       │   │       ├── futures-macro v0.3.31 (proc-macro)
│   │       │   │       │   ├── proc-macro2 v1.0.101 (*)
│   │       │   │       │   ├── quote v1.0.41 (*)
│   │       │   │       │   └── syn v2.0.106 (*)
│   │       │   │       ├── futures-sink v0.3.31
│   │       │   │       ├── futures-task v0.3.31
│   │       │   │       ├── memchr v2.7.6
│   │       │   │       ├── pin-project-lite v0.2.16
│   │       │   │       ├── pin-utils v0.1.0
│   │       │   │       └── slab v0.4.11
│   │       │   ├── futures-io v0.3.31
│   │       │   ├── futures-sink v0.3.31
│   │       │   ├── futures-task v0.3.31
│   │       │   └── futures-util v0.3.31 (*)
│   │       ├── futures-util v0.3.31 (*)
│   │       ├── libc v0.2.176
│   │       ├── metrics v0.21.1
│   │       │   ├── ahash v0.8.12
│   │       │   │   ├── cfg-if v1.0.3
│   │       │   │   ├── getrandom v0.3.3
│   │       │   │   │   ├── cfg-if v1.0.3
│   │       │   │   │   └── libc v0.2.176
│   │       │   │   ├── once_cell v1.21.3
│   │       │   │   └── zerocopy v0.8.27
│   │       │   │   [build-dependencies]
│   │       │   │   └── version_check v0.9.5
│   │       │   └── metrics-macros v0.7.1 (proc-macro)
│   │       │       ├── proc-macro2 v1.0.101 (*)
│   │       │       ├── quote v1.0.41 (*)
│   │       │       └── syn v2.0.106 (*)
│   │       ├── mio v0.8.11
│   │       │   ├── libc v0.2.176
│   │       │   └── log v0.4.28
│   │       ├── nix v0.27.1
│   │       │   ├── bitflags v2.9.4
│   │       │   ├── cfg-if v1.0.3
│   │       │   └── libc v0.2.176
│   │       ├── pin-project v1.1.10
│   │       │   └── pin-project-internal v1.1.10 (proc-macro)
│   │       │       ├── proc-macro2 v1.0.101 (*)
│   │       │       ├── quote v1.0.41 (*)
│   │       │       └── syn v2.0.106 (*)
│   │       ├── pin-project-lite v0.2.16
│   │       ├── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
│   │       └── socket2 v0.5.10
│   │           └── libc v0.2.176
│   ├── serde v1.0.228 (*)
│   └── thiserror v1.0.69 (*)
├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
├── foundation_telemetry v0.1.0 (/workspace/the-block/crates/foundation_telemetry)
│   └── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
├── futures v0.3.31 (*)
├── governance v0.1.0 (/workspace/the-block/governance)
│   ├── foundation_lazy v0.1.0 (/workspace/the-block/crates/foundation_lazy)
│   ├── foundation_math v0.1.0 (/workspace/the-block/crates/foundation_math)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
│   ├── serde v1.0.228 (*)
│   └── sled v0.34.0 (/workspace/the-block/sled)
│       ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│       ├── foundation_lazy v0.1.0 (/workspace/the-block/crates/foundation_lazy)
│       ├── storage_engine v0.1.0 (/workspace/the-block/crates/storage_engine) (*)
│       ├── tempfile v3.23.0
│       │   ├── fastrand v2.3.0
│       │   ├── getrandom v0.3.3 (*)
│       │   ├── once_cell v1.21.3
│       │   └── rustix v1.1.2
│       │       ├── bitflags v2.9.4
│       │       └── linux-raw-sys v0.11.0
│       └── thiserror v1.0.69 (*)
├── histogram_fp v0.1.0 (/workspace/the-block/crates/histogram_fp)
├── httpd v0.1.0 (/workspace/the-block/crates/httpd) (*)
├── foundation_unicode v0.1.0 (/workspace/the-block/crates/foundation_unicode)
│   ├── smallvec v1.15.1
│   ├── utf16_iter v1.0.5
│   ├── utf8_iter v1.0.4
│   ├── write16 v1.0.0
│   └── zerovec v0.11.4 (*)
├── inflation v0.1.0 (/workspace/the-block/inflation)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   └── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
├── jurisdiction v0.1.0 (/workspace/the-block/crates/jurisdiction)
│   ├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── foundation_lazy v0.1.0 (/workspace/the-block/crates/foundation_lazy)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── httpd v0.1.0 (/workspace/the-block/crates/httpd) (*)
│   ├── log v0.4.28
│   └── serde v1.0.228 (*)
├── ledger v0.1.0 (/workspace/the-block/ledger) (*)
├── light-client v0.1.0 (/workspace/the-block/crates/light-client)
│   ├── coding v0.1.0 (/workspace/the-block/crates/coding) (*)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── foundation_archive v0.1.0 (/workspace/the-block/crates/foundation_archive)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── futures v0.3.31 (*)
│   ├── runtime v0.1.0 (/workspace/the-block/crates/runtime) (*)
│   ├── state v0.1.0 (/workspace/the-block/state)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   ├── storage_engine v0.1.0 (/workspace/the-block/crates/storage_engine) (*)
│   │   ├── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
│   │   └── thiserror v1.0.69 (*)
│   │   [build-dependencies]
│   │   └── dependency_guard v0.1.0 (/workspace/the-block/crates/dependency_guard)
│   ├── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
│   ├── thiserror v1.0.69 (*)
│   └── tracing v0.1.41
│       ├── pin-project-lite v0.2.16
│       ├── tracing-attributes v0.1.30 (proc-macro)
│       │   ├── proc-macro2 v1.0.101 (*)
│       │   ├── quote v1.0.41 (*)
│       │   └── syn v2.0.106 (*)
│       └── tracing-core v0.1.34
│           └── once_cell v1.21.3
├── p2p_overlay v0.1.0 (/workspace/the-block/crates/p2p_overlay)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   └── serde v1.0.228 (*)
├── pprof v0.13.0
│   ├── backtrace v0.3.76
│   │   ├── addr2line v0.25.1
│   │   │   └── gimli v0.32.3
│   │   ├── cfg-if v1.0.3
│   │   ├── libc v0.2.176
│   │   ├── miniz_oxide v0.8.9
│   │   │   └── adler2 v2.0.1
│   │   ├── object v0.37.3
│   │   │   └── memchr v2.7.6
│   │   └── rustc-demangle v0.1.26
│   ├── cfg-if v1.0.3
│   ├── findshlibs v0.10.2
│   │   └── libc v0.2.176
│   │   [build-dependencies]
│   │   └── cc v1.2.40
│   │       ├── find-msvc-tools v0.1.3
│   │       └── shlex v1.3.0
│   ├── inferno v0.11.21
│   │   ├── ahash v0.8.12 (*)
│   │   ├── indexmap v2.11.4
│   │   │   ├── equivalent v1.0.2
│   │   │   └── hashbrown v0.16.0
│   │   ├── is-terminal v0.4.16
│   │   │   └── libc v0.2.176
│   │   ├── itoa v1.0.15
│   │   ├── log v0.4.28
│   │   ├── num-format v0.4.4
│   │   │   ├── arrayvec v0.7.6
│   │   │   └── itoa v1.0.15
│   │   ├── once_cell v1.21.3
│   │   ├── quick-xml v0.26.0
│   │   │   └── memchr v2.7.6
│   │   ├── rgb v0.8.52
│   │   │   └── bytemuck v1.24.0
│   │   └── str_stack v0.1.0
│   ├── libc v0.2.176
│   ├── log v0.4.28
│   ├── nix v0.26.4
│   │   ├── bitflags v1.3.2
│   │   ├── cfg-if v1.0.3
│   │   └── libc v0.2.176
│   ├── once_cell v1.21.3
│   ├── parking_lot v0.12.5
│   │   ├── lock_api v0.4.14
│   │   │   └── scopeguard v1.2.0
│   │   └── parking_lot_core v0.9.12
│   │       ├── cfg-if v1.0.3
│   │       ├── libc v0.2.176
│   │       └── smallvec v1.15.1
│   ├── smallvec v1.15.1
│   ├── symbolic-demangle v12.16.3
│   │   ├── cpp_demangle v0.4.5
│   │   │   └── cfg-if v1.0.3
│   │   ├── rustc-demangle v0.1.26
│   │   └── symbolic-common v12.16.3
│   │       ├── debugid v0.8.0
│   │       │   └── uuid v1.18.1
│   │       │       └── getrandom v0.3.3 (*)
│   │       ├── memmap2 v0.9.8
│   │       │   └── libc v0.2.176
│   │       ├── stable_deref_trait v1.2.0
│   │       └── uuid v1.18.1 (*)
│   ├── tempfile v3.23.0 (*)
│   └── thiserror v1.0.69 (*)
├── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
├── runtime v0.1.0 (/workspace/the-block/crates/runtime) (*)
├── serde v1.0.228 (*)
├── serde_bytes v0.11.19 (*)
├── sled v0.34.0 (/workspace/the-block/sled) (*)
├── state v0.1.0 (/workspace/the-block/state) (*)
├── static_assertions v1.1.0
├── storage v0.1.0 (/workspace/the-block/storage)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── serde v1.0.228 (*)
│   └── thiserror v1.0.69 (*)
├── storage_engine v0.1.0 (/workspace/the-block/crates/storage_engine) (*)
├── subtle v2.6.1
├── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
├── time v0.3.44
│   ├── deranged v0.5.4
│   │   └── powerfmt v0.2.0
│   ├── itoa v1.0.15
│   ├── num-conv v0.1.0
│   ├── powerfmt v0.2.0
│   ├── time-core v0.1.6
│   └── time-macros v0.2.24 (proc-macro)
│       ├── num-conv v0.1.0
│       └── time-core v0.1.6
├── wallet v0.1.0 (/workspace/the-block/crates/wallet)
│   ├── base64_fp v0.1.0 (/workspace/the-block/crates/base64_fp)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── foundation_lazy v0.1.0 (/workspace/the-block/crates/foundation_lazy)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── httpd v0.1.0 (/workspace/the-block/crates/httpd) (*)
│   ├── ledger v0.1.0 (/workspace/the-block/ledger) (*)
│   ├── metrics v0.21.1 (*)
│   ├── native-tls v0.2.14
│   │   ├── log v0.4.28
│   │   ├── openssl v0.10.73
│   │   │   ├── bitflags v2.9.4
│   │   │   ├── cfg-if v1.0.3
│   │   │   ├── foreign-types v0.3.2
│   │   │   │   └── foreign-types-shared v0.1.1
│   │   │   ├── libc v0.2.176
│   │   │   ├── once_cell v1.21.3
│   │   │   ├── openssl-macros v0.1.1 (proc-macro)
│   │   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   │   ├── quote v1.0.41 (*)
│   │   │   │   └── syn v2.0.106 (*)
│   │   │   └── openssl-sys v0.9.109
│   │   │       └── libc v0.2.176
│   │   │       [build-dependencies]
│   │   │       ├── cc v1.2.40 (*)
│   │   │       ├── pkg-config v0.3.32
│   │   │       └── vcpkg v0.2.15
│   │   ├── openssl-probe v0.1.6
│   │   └── openssl-sys v0.9.109 (*)
│   ├── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
│   ├── serde v1.0.228 (*)
│   ├── subtle v2.6.1
│   ├── thiserror v1.0.69 (*)
│   ├── tracing v0.1.41 (*)
│   └── uuid v1.18.1 (*)
└── x509-parser v0.16.0
    ├── asn1-rs v0.6.2
    │   ├── asn1-rs-derive v0.5.1 (proc-macro)
    │   │   ├── proc-macro2 v1.0.101 (*)
    │   │   ├── quote v1.0.41 (*)
    │   │   ├── syn v2.0.106 (*)
    │   │   └── synstructure v0.13.2 (*)
    │   ├── asn1-rs-impl v0.2.0 (proc-macro)
    │   │   ├── proc-macro2 v1.0.101 (*)
    │   │   ├── quote v1.0.41 (*)
    │   │   └── syn v2.0.106 (*)
    │   ├── displaydoc v0.2.5 (proc-macro) (*)
    │   ├── nom v7.1.3
    │   │   ├── memchr v2.7.6
    │   │   └── minimal-lexical v0.2.1
    │   ├── num-traits v0.2.19 (*)
    │   ├── rusticata-macros v4.1.0
    │   │   └── nom v7.1.3 (*)
    │   ├── thiserror v1.0.69 (*)
    │   └── time v0.3.44 (*)
    ├── data-encoding v2.9.0
    ├── der-parser v9.0.0
    │   ├── asn1-rs v0.6.2 (*)
    │   ├── displaydoc v0.2.5 (proc-macro) (*)
    │   ├── nom v7.1.3 (*)
    │   ├── num-bigint v0.4.6 (*)
    │   ├── num-traits v0.2.19 (*)
    │   └── rusticata-macros v4.1.0 (*)
    ├── lazy_static v1.5.0
    ├── nom v7.1.3 (*)
    ├── oid-registry v0.7.1
    │   └── asn1-rs v0.6.2 (*)
    ├── rusticata-macros v4.1.0 (*)
    ├── thiserror v1.0.69 (*)
    └── time v0.3.44 (*)
[build-dependencies]
├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
└── dependency_guard v0.1.0 (/workspace/the-block/crates/dependency_guard)
[dev-dependencies]
├── jurisdiction v0.1.0 (/workspace/the-block/crates/jurisdiction) (*)
├── tb-sim v0.1.0 (/workspace/the-block/sim)
│   ├── anyhow v1.0.100
│   ├── cli_core v0.1.0 (/workspace/the-block/crates/cli_core) (*)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── csv v1.3.1
│   │   ├── csv-core v0.1.12
│   │   │   └── memchr v2.7.6
│   │   ├── itoa v1.0.15
│   │   ├── ryu v1.0.20
│   │   └── serde v1.0.228 (*)
│   ├── dex v0.1.0 (/workspace/the-block/dex) (*)
│   ├── dkg v0.1.0 (/workspace/the-block/dkg) (*)
│   ├── explorer v0.1.0 (/workspace/the-block/explorer)
│   │   ├── anyhow v1.0.100
│   │   ├── codec v0.1.0 (/workspace/the-block/crates/codec) (*)
│   │   ├── concurrency v0.1.0 (/workspace/the-block/crates/concurrency) (*)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   │   ├── httpd v0.1.0 (/workspace/the-block/crates/httpd) (*)
│   │   ├── runtime v0.1.0 (/workspace/the-block/crates/runtime) (*)
│   │   ├── foundation_sqlite v0.1.0 (/workspace/the-block/crates/foundation_sqlite)
│   │   │   └── rusqlite v0.30.0
│   │   │       ├── bitflags v2.9.4
│   │   │       ├── fallible-iterator v0.3.0
│   │   │       ├── fallible-streaming-iterator v0.1.9
│   │   │       ├── hashlink v0.8.4
│   │   │       │   └── hashbrown v0.14.5
│   │   │       │       ├── ahash v0.8.12 (*)
│   │   │       │       └── allocator-api2 v0.2.21
│   │   │       ├── libsqlite3-sys v0.27.0
│   │   │       │   [build-dependencies]
│   │   │       │   ├── cc v1.2.40 (*)
│   │   │       │   ├── pkg-config v0.3.32
│   │   │       │   └── vcpkg v0.2.15
│   │   │       └── smallvec v1.15.1
│   │   ├── serde v1.0.228 (*)
│   │   ├── storage v0.1.0 (/workspace/the-block/storage) (*)
│   │   ├── sys v0.1.0 (/workspace/the-block/crates/sys) (*)
│   │   └── the_block v0.1.0 (/workspace/the-block/node) (*)
│   ├── foundation_lazy v0.1.0 (/workspace/the-block/crates/foundation_lazy)
│   ├── foundation_serialization v0.1.0 (/workspace/the-block/crates/foundation_serialization) (*)
│   ├── ledger v0.1.0 (/workspace/the-block/ledger) (*)
│   ├── light-client v0.1.0 (/workspace/the-block/crates/light-client) (*)
│   ├── rand v0.1.0 (/workspace/the-block/crates/rand) (*)
│   ├── serde v1.0.228 (*)
│   ├── tempfile v3.23.0 (*)
│   ├── the_block v0.1.0 (/workspace/the-block/node) (*)
│   └── thiserror v1.0.69 (*)
└── testkit v0.1.0 (/workspace/the-block/crates/testkit)
    └── testkit_macros v0.1.0 (proc-macro) (/workspace/the-block/crates/testkit_macros)
        ├── proc-macro2 v1.0.101 (*)
        ├── quote v1.0.41 (*)
        └── syn v2.0.106 (*)
```
