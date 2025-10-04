# Node Dependency Tree
> **Review (2025-10-01):** Removed ed25519-dalek/blake3/sha3/bellman_ce entries and noted first-party crypto suite dependencies.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25). Runtime-native WebSockets (`runtime::ws`) now back `/logs/tail`, `/state/stream`, `/vm.trace`, the gateway peer-metrics feed, and CLI consumers, eliminating the `tokio-tungstenite`/`hyper-tungstenite` stack across the workspace (2025-10-02).

This document lists the dependency hierarchy for the `the_block` node crate. It is generated via `cargo tree --manifest-path node/Cargo.toml`.

> **Update (2025-11-05):** The workspace now relies on the first-party
> `httpd` crate for outbound HTTP/JSON traffic. The tables below still show
> legacy `reqwest` entries from the previous capture; treat them as historical
> until the next tree export lands.

> **Update (2025-11-15):** Tests, examples, and tooling mocks have been
> consolidated on the in-house `httpd` harness. Legacy mentions of
> `tiny_http`, `axum`, and related dev-dependencies have been pruned from this
> snapshot.

```
the_block v0.1.0 (/workspace/the-block/node)
├── anyhow v1.0.100
├── base64 v0.22.1
├── base64ct v1.8.0
├── bincode v1.3.3
│   └── serde v1.0.228
│       ├── serde_core v1.0.228
│       └── serde_derive v1.0.228 (proc-macro)
│           ├── proc-macro2 v1.0.101
│           │   └── unicode-ident v1.0.19
│           ├── quote v1.0.41
│           │   └── proc-macro2 v1.0.101 (*)
│           └── syn v2.0.106
│               ├── proc-macro2 v1.0.101 (*)
│               ├── quote v1.0.41 (*)
│               └── unicode-ident v1.0.19
├── bridges v0.1.0 (/workspace/the-block/bridges)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite)
│   │   ├── bincode v1.3.3 (*)
│   │   ├── codec v0.1.0 (/workspace/the-block/crates/codec)
│   │   │   ├── bincode v1.3.3 (*)
│   │   │   ├── serde v1.0.228 (*)
│   │   │   ├── serde_cbor v0.11.2
│   │   │   │   ├── half v1.8.3
│   │   │   │   └── serde v1.0.228 (*)
│   │   │   ├── serde_json v1.0.145
│   │   │   │   ├── itoa v1.0.15
│   │   │   │   ├── memchr v2.7.6
│   │   │   │   ├── ryu v1.0.20
│   │   │   │   └── serde_core v1.0.228
│   │   │   └── thiserror v1.0.69
│   │   │       └── thiserror-impl v1.0.69 (proc-macro)
│   │   │           ├── proc-macro2 v1.0.101 (*)
│   │   │           ├── quote v1.0.41 (*)
│   │   │           └── syn v2.0.106 (*)
│   │   ├── hex v0.4.3
│   │   ├── num-bigint v0.4.6
│   │   │   ├── num-integer v0.1.46
│   │   │   │   └── num-traits v0.2.19
│   │   │   │       └── libm v0.2.15
│   │   │   │       [build-dependencies]
│   │   │   │       └── autocfg v1.5.0
│   │   │   └── num-traits v0.2.19 (*)
│   │   ├── num-traits v0.2.19 (*)
│   │   ├── once_cell v1.21.3
│   │   ├── rand v0.4.6
│   │   │   └── libc v0.2.176
│   │   ├── rand v0.8.5
│   │   │   ├── libc v0.2.176
│   │   │   ├── rand_chacha v0.3.1
│   │   │   │   ├── ppv-lite86 v0.2.21
│   │   │   │   │   └── zerocopy v0.8.27
│   │   │   │   └── rand_core v0.6.4
│   │   │   │       └── getrandom v0.2.16
│   │   │   │           ├── cfg-if v1.0.3
│   │   │   │           └── libc v0.2.176
│   │   │   └── rand_core v0.6.4 (*)
│   │   ├── rand_core v0.6.4 (*)
│   │   ├── serde v1.0.228 (*)
│   │   └── thiserror v1.0.69 (*)
│   ├── hex v0.4.3
│   ├── ledger v0.1.0 (/workspace/the-block/ledger)
│   │   ├── bincode v1.3.3 (*)
│   │   ├── clap v4.5.48
│   │   │   ├── clap_builder v4.5.48
│   │   │   │   ├── anstream v0.6.20
│   │   │   │   │   ├── anstyle v1.0.13
│   │   │   │   │   ├── anstyle-parse v0.2.7
│   │   │   │   │   │   └── utf8parse v0.2.2
│   │   │   │   │   ├── anstyle-query v1.1.4
│   │   │   │   │   ├── colorchoice v1.0.4
│   │   │   │   │   ├── is_terminal_polyfill v1.70.1
│   │   │   │   │   └── utf8parse v0.2.2
│   │   │   │   ├── anstyle v1.0.13
│   │   │   │   ├── clap_lex v0.7.5
│   │   │   │   └── strsim v0.11.1
│   │   │   └── clap_derive v4.5.47 (proc-macro)
│   │   │       ├── heck v0.5.0
│   │   │       ├── proc-macro2 v1.0.101 (*)
│   │   │       ├── quote v1.0.41 (*)
│   │   │       └── syn v2.0.106 (*)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   ├── serde v1.0.228 (*)
│   │   ├── serde_json v1.0.145 (*)
│   │   └── storage_engine v0.1.0 (/workspace/the-block/crates/storage_engine)
│   │       ├── base64 v0.22.1
│   │       ├── bincode v1.3.3 (*)
│   │       ├── parking_lot v0.12.4
│   │       │   ├── lock_api v0.4.13
│   │       │   │   └── scopeguard v1.2.0
│   │       │   │   [build-dependencies]
│   │       │   │   └── autocfg v1.5.0
│   │       │   └── parking_lot_core v0.9.11
│   │       │       ├── cfg-if v1.0.3
│   │       │       ├── libc v0.2.176
│   │       │       └── smallvec v1.15.1
│   │       │           └── serde v1.0.228 (*)
│   │       ├── rocksdb v0.21.0 (/workspace/the-block/vendor/rocksdb-0.21.0/rocksdb-0.21.0)
│   │       │   ├── libc v0.2.176
│   │       │   └── librocksdb-sys v0.11.0+8.1.1
│   │       │       ├── bzip2-sys v0.1.13+1.0.8
│   │       │       │   [build-dependencies]
│   │       │       │   ├── cc v1.2.39
│   │       │       │   │   ├── find-msvc-tools v0.1.2
│   │       │       │   │   ├── jobserver v0.1.34
│   │       │       │   │   │   └── libc v0.2.176
│   │       │       │   │   ├── libc v0.2.176
│   │       │       │   │   └── shlex v1.3.0
│   │       │       │   └── pkg-config v0.3.32
│   │       │       ├── libc v0.2.176
│   │       │       ├── libz-sys v1.1.22
│   │       │       │   [build-dependencies]
│   │       │       │   ├── cc v1.2.39 (*)
│   │       │       │   ├── pkg-config v0.3.32
│   │       │       │   └── vcpkg v0.2.15
│   │       │       ├── lz4-sys v1.11.1+lz4-1.10.0
│   │       │       │   └── libc v0.2.176
│   │       │       │   [build-dependencies]
│   │       │       │   └── cc v1.2.39 (*)
│   │       │       └── zstd-sys v2.0.16+zstd.1.5.7
│   │       │           [build-dependencies]
│   │       │           ├── bindgen v0.72.1
│   │       │           │   ├── bitflags v2.9.4
│   │       │           │   ├── cexpr v0.6.0
│   │       │           │   │   └── nom v7.1.3
│   │       │           │   │       ├── memchr v2.7.6
│   │       │           │   │       └── minimal-lexical v0.2.1
│   │       │           │   ├── clang-sys v1.8.1
│   │       │           │   │   ├── glob v0.3.3
│   │       │           │   │   ├── libc v0.2.176
│   │       │           │   │   └── libloading v0.8.9
│   │       │           │   │       └── cfg-if v1.0.3
│   │       │           │   │   [build-dependencies]
│   │       │           │   │   └── glob v0.3.3
│   │       │           │   ├── itertools v0.13.0
│   │       │           │   │   └── either v1.15.0
│   │       │           │   ├── proc-macro2 v1.0.101 (*)
│   │       │           │   ├── quote v1.0.41 (*)
│   │       │           │   ├── regex v1.11.3
│   │       │           │   │   ├── regex-automata v0.4.11
│   │       │           │   │   │   └── regex-syntax v0.8.6
│   │       │           │   │   └── regex-syntax v0.8.6
│   │       │           │   ├── rustc-hash v2.1.1
│   │       │           │   ├── shlex v1.3.0
│   │       │           │   └── syn v2.0.106 (*)
│   │       │           ├── cc v1.2.39 (*)
│   │       │           └── pkg-config v0.3.32
│   │       │       [build-dependencies]
│   │       │       ├── bindgen v0.65.1
│   │       │       │   ├── bitflags v1.3.2
│   │       │       │   ├── cexpr v0.6.0 (*)
│   │       │       │   ├── clang-sys v1.8.1 (*)
│   │       │       │   ├── lazy_static v1.5.0
│   │       │       │   ├── lazycell v1.3.0
│   │       │       │   ├── peeking_take_while v0.1.2
│   │       │       │   ├── prettyplease v0.2.37
│   │       │       │   │   ├── proc-macro2 v1.0.101 (*)
│   │       │       │   │   └── syn v2.0.106 (*)
│   │       │       │   ├── proc-macro2 v1.0.101 (*)
│   │       │       │   ├── quote v1.0.41 (*)
│   │       │       │   ├── regex v1.11.3 (*)
│   │       │       │   ├── rustc-hash v1.1.0
│   │       │       │   ├── shlex v1.3.0
│   │       │       │   └── syn v2.0.106 (*)
│   │       │       ├── cc v1.2.39 (*)
│   │       │       └── glob v0.3.3
│   │       ├── serde v1.0.228 (*)
│   │       ├── serde_json v1.0.145 (*)
│   │       ├── sled v0.34.7
│   │       │   ├── crc32fast v1.5.0
│   │       │   │   └── cfg-if v1.0.3
│   │       │   ├── crossbeam-epoch v0.9.18
│   │       │   │   └── crossbeam-utils v0.8.21
│   │       │   ├── crossbeam-utils v0.8.21
│   │       │   ├── fs2 v0.4.3
│   │       │   │   └── libc v0.2.176
│   │       │   ├── fxhash v0.2.1
│   │       │   │   └── byteorder v1.5.0
│   │       │   ├── libc v0.2.176
│   │       │   ├── log v0.4.28
│   │       │   │   └── value-bag v1.11.1
│   │       │   └── parking_lot v0.11.2
│   │       │       ├── instant v0.1.13
│   │       │       │   └── cfg-if v1.0.3
│   │       │       ├── lock_api v0.4.13 (*)
│   │       │       └── parking_lot_core v0.8.6
│   │       │           ├── cfg-if v1.0.3
│   │       │           ├── instant v0.1.13 (*)
│   │       │           ├── libc v0.2.176
│   │       │           └── smallvec v1.15.1 (*)
│   │       ├── tempfile v3.23.0
│   │       │   ├── fastrand v2.3.0
│   │       │   ├── getrandom v0.3.3
│   │       │   │   ├── cfg-if v1.0.3
│   │       │   │   └── libc v0.2.176
│   │       │   ├── once_cell v1.21.3
│   │       │   └── rustix v1.1.2
│   │       │       ├── bitflags v2.9.4
│   │       │       └── linux-raw-sys v0.11.0
│   │       └── thiserror v1.0.69 (*)
│   ├── serde v1.0.228 (*)
│   └── serde_json v1.0.145 (*)
├── bytes v1.10.1
├── clap v4.5.48 (*)
├── clap_complete v4.5.58
│   └── clap v4.5.48 (*)
├── codec v0.1.0 (/workspace/the-block/crates/codec) (*)
├── coding v0.1.0 (/workspace/the-block/crates/coding)
│   ├── rand v0.8.5 (*)
│   ├── serde v1.0.228 (*)
│   ├── thiserror v1.0.69 (*)
│   └── toml v0.8.23
│       ├── serde v1.0.228 (*)
│       ├── serde_spanned v0.6.9
│       │   └── serde v1.0.228 (*)
│       ├── toml_datetime v0.6.11
│       │   └── serde v1.0.228 (*)
│       └── toml_edit v0.22.27
│           ├── indexmap v2.11.4
│           │   ├── equivalent v1.0.2
│           │   ├── hashbrown v0.16.0
│           │   └── serde_core v1.0.228
│           ├── serde v1.0.228 (*)
│           ├── serde_spanned v0.6.9 (*)
│           ├── toml_datetime v0.6.11 (*)
│           ├── toml_write v0.1.2
│           └── winnow v0.7.13
├── colored v2.2.0
│   └── lazy_static v1.5.0
├── crc32fast v1.5.0 (*)
├── crypto v0.1.0 (/workspace/the-block/crypto)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   └── rand v0.8.5 (*)
├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
├── dashmap v5.5.3
│   ├── cfg-if v1.0.3
│   ├── hashbrown v0.14.5
│   │   ├── ahash v0.8.12
│   │   │   ├── cfg-if v1.0.3
│   │   │   ├── getrandom v0.3.3 (*)
│   │   │   ├── once_cell v1.21.3
│   │   │   └── zerocopy v0.8.27
│   │   │   [build-dependencies]
│   │   │   └── version_check v0.9.5
│   │   ├── allocator-api2 v0.2.21
│   │   └── serde v1.0.228 (*)
│   ├── lock_api v0.4.13 (*)
│   ├── once_cell v1.21.3
│   └── parking_lot_core v0.9.11 (*)
├── dex v0.1.0 (/workspace/the-block/dex)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── hex v0.4.3
│   ├── serde v1.0.228 (*)
│   └── subtle v2.6.1
├── dirs v5.0.1
│   └── dirs-sys v0.4.1
│       ├── libc v0.2.176
│       └── option-ext v0.2.0
├── dkg v0.1.0 (/workspace/the-block/dkg)
│   ├── rand v0.7.3
│   │   ├── getrandom v0.1.16
│   │   │   ├── cfg-if v1.0.3
│   │   │   └── libc v0.2.176
│   │   ├── libc v0.2.176
│   │   ├── rand_chacha v0.2.2
│   │   │   ├── ppv-lite86 v0.2.21 (*)
│   │   │   └── rand_core v0.5.1
│   │   │       └── getrandom v0.1.16 (*)
│   │   └── rand_core v0.5.1 (*)
│   └── threshold_crypto v0.4.0
│       ├── byteorder v1.5.0
│       ├── failure v0.1.8
│       │   ├── backtrace v0.3.76
│       │   │   ├── addr2line v0.25.1
│       │   │   │   └── gimli v0.32.3
│       │   │   ├── cfg-if v1.0.3
│       │   │   ├── libc v0.2.176
│       │   │   ├── miniz_oxide v0.8.9
│       │   │   │   └── adler2 v2.0.1
│       │   │   ├── object v0.37.3
│       │   │   │   └── memchr v2.7.6
│       │   │   └── rustc-demangle v0.1.26
│       │   └── failure_derive v0.1.8 (proc-macro)
│       │       ├── proc-macro2 v1.0.101 (*)
│       │       ├── quote v1.0.41 (*)
│       │       ├── syn v1.0.109
│       │       │   ├── proc-macro2 v1.0.101 (*)
│       │       │   ├── quote v1.0.41 (*)
│       │       │   └── unicode-ident v1.0.19
│       │       └── synstructure v0.12.6
│       │           ├── proc-macro2 v1.0.101 (*)
│       │           ├── quote v1.0.41 (*)
│       │           ├── syn v1.0.109 (*)
│       │           └── unicode-xid v0.2.6
│       ├── ff v0.6.0
│       │   ├── byteorder v1.5.0
│       │   ├── ff_derive v0.6.0 (proc-macro)
│       │   │   ├── num-bigint v0.2.6
│       │   │   │   ├── num-integer v0.1.46
│       │   │   │   │   └── num-traits v0.2.19
│       │   │   │   │       [build-dependencies]
│       │   │   │   │       └── autocfg v1.5.0
│       │   │   │   └── num-traits v0.2.19 (*)
│       │   │   │   [build-dependencies]
│       │   │   │   └── autocfg v1.5.0
│       │   │   ├── num-integer v0.1.46 (*)
│       │   │   ├── num-traits v0.2.19 (*)
│       │   │   ├── proc-macro2 v1.0.101 (*)
│       │   │   ├── quote v1.0.41 (*)
│       │   │   └── syn v1.0.109 (*)
│       │   └── rand_core v0.5.1 (*)
│       ├── group v0.6.0
│       │   ├── ff v0.6.0 (*)
│       │   ├── rand v0.7.3 (*)
│       │   └── rand_xorshift v0.2.0
│       │       └── rand_core v0.5.1 (*)
│       ├── hex_fmt v0.3.0
│       ├── log v0.4.28 (*)
│       ├── pairing v0.16.0
│       │   ├── byteorder v1.5.0
│       │   ├── ff v0.6.0 (*)
│       │   ├── group v0.6.0 (*)
│       │   └── rand_core v0.5.1 (*)
│       ├── rand v0.7.3 (*)
│       ├── rand_chacha v0.2.2 (*)
│       ├── serde v1.0.228 (*)
│       ├── tiny-keccak v2.0.2
│       │   └── crunchy v0.2.4
│       └── zeroize v1.8.2
├── flate2 v1.1.2
│   ├── crc32fast v1.5.0 (*)
│   └── miniz_oxide v0.8.9 (*)
├── fs2 v0.4.3 (*)
├── futures v0.3.31
│   ├── futures-channel v0.3.31
│   │   ├── futures-core v0.3.31
│   │   └── futures-sink v0.3.31
│   ├── futures-core v0.3.31
│   ├── futures-executor v0.3.31
│   │   ├── futures-core v0.3.31
│   │   ├── futures-task v0.3.31
│   │   └── futures-util v0.3.31
│   │       ├── futures-channel v0.3.31 (*)
│   │       ├── futures-core v0.3.31
│   │       ├── futures-io v0.3.31
│   │       ├── futures-macro v0.3.31 (proc-macro)
│   │       │   ├── proc-macro2 v1.0.101 (*)
│   │       │   ├── quote v1.0.41 (*)
│   │       │   └── syn v2.0.106 (*)
│   │       ├── futures-sink v0.3.31
│   │       ├── futures-task v0.3.31
│   │       ├── memchr v2.7.6
│   │       ├── pin-project-lite v0.2.16
│   │       ├── pin-utils v0.1.0
│   │       └── slab v0.4.11
│   ├── futures-io v0.3.31
│   ├── futures-sink v0.3.31
│   ├── futures-task v0.3.31
│   └── futures-util v0.3.31 (*)
├── governance v0.1.0 (/workspace/the-block/governance)
│   ├── bincode v1.3.3 (*)
│   ├── nalgebra v0.32.6
│   │   ├── approx v0.5.1
│   │   │   └── num-traits v0.2.19 (*)
│   │   ├── matrixmultiply v0.3.10
│   │   │   └── rawpointer v0.2.1
│   │   │   [build-dependencies]
│   │   │   └── autocfg v1.5.0
│   │   ├── nalgebra-macros v0.2.2 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   ├── quote v1.0.41 (*)
│   │   │   └── syn v2.0.106 (*)
│   │   ├── num-complex v0.4.6
│   │   │   └── num-traits v0.2.19 (*)
│   │   ├── num-rational v0.4.2
│   │   │   ├── num-integer v0.1.46 (*)
│   │   │   └── num-traits v0.2.19 (*)
│   │   ├── num-traits v0.2.19 (*)
│   │   ├── simba v0.8.1
│   │   │   ├── approx v0.5.1 (*)
│   │   │   ├── num-complex v0.4.6 (*)
│   │   │   ├── num-traits v0.2.19 (*)
│   │   │   ├── paste v1.0.15 (proc-macro)
│   │   │   └── wide v0.7.33
│   │   │       ├── bytemuck v1.23.2
│   │   │       └── safe_arch v0.7.4
│   │   │           └── bytemuck v1.23.2
│   │   └── typenum v1.18.0
│   ├── once_cell v1.21.3
│   ├── rand v0.8.5 (*)
│   ├── rustdct v0.7.1
│   │   └── rustfft v6.4.1
│   │       ├── num-complex v0.4.6 (*)
│   │       ├── num-integer v0.1.46 (*)
│   │       ├── num-traits v0.2.19 (*)
│   │       ├── primal-check v0.3.4
│   │       │   └── num-integer v0.1.46 (*)
│   │       ├── strength_reduce v0.2.4
│   │       └── transpose v0.2.3
│   │           ├── num-integer v0.1.46 (*)
│   │           └── strength_reduce v0.2.4
│   ├── serde v1.0.228 (*)
│   ├── serde_json v1.0.145 (*)
│   ├── sled v0.34.7 (*)
│   └── statrs v0.16.1
│       ├── approx v0.5.1 (*)
│       ├── lazy_static v1.5.0
│       ├── nalgebra v0.29.0
│       │   ├── approx v0.5.1 (*)
│       │   ├── matrixmultiply v0.3.10 (*)
│       │   ├── nalgebra-macros v0.1.0 (proc-macro)
│       │   │   ├── proc-macro2 v1.0.101 (*)
│       │   │   ├── quote v1.0.41 (*)
│       │   │   └── syn v1.0.109 (*)
│       │   ├── num-complex v0.4.6 (*)
│       │   ├── num-rational v0.4.2 (*)
│       │   ├── num-traits v0.2.19 (*)
│       │   ├── rand v0.8.5 (*)
│       │   ├── rand_distr v0.4.3
│       │   │   ├── num-traits v0.2.19 (*)
│       │   │   └── rand v0.8.5 (*)
│       │   ├── simba v0.6.0
│       │   │   ├── approx v0.5.1 (*)
│       │   │   ├── num-complex v0.4.6 (*)
│       │   │   ├── num-traits v0.2.19 (*)
│       │   │   ├── paste v1.0.15 (proc-macro)
│       │   │   └── wide v0.7.33 (*)
│       │   └── typenum v1.18.0
│       ├── num-traits v0.2.19 (*)
│       └── rand v0.8.5 (*)
├── hdrhistogram v7.5.4
│   ├── base64 v0.21.7
│   ├── byteorder v1.5.0
│   ├── crossbeam-channel v0.5.15
│   │   └── crossbeam-utils v0.8.21
│   ├── flate2 v1.1.2 (*)
│   ├── nom v7.1.3
│   │   ├── memchr v2.7.6
│   │   └── minimal-lexical v0.2.1
│   └── num-traits v0.2.19 (*)
├── hex v0.4.3
├── httparse v1.10.1
├── httpd v0.1.0 (/workspace/the-block/crates/httpd)
│   ├── codec v0.1.0 (/workspace/the-block/crates/codec) (*)
│   ├── runtime v0.1.0 (/workspace/the-block/crates/runtime)
│   │   ├── base64 v0.21.7
│   │   ├── crossbeam-deque v0.8.6
│   │   │   ├── crossbeam-epoch v0.9.18 (*)
│   │   │   └── crossbeam-utils v0.8.21
│   │   ├── futures v0.3.31 (*)
│   │   ├── futures-util v0.3.31 (*)
│   │   ├── libc v0.2.176
│   │   ├── metrics v0.21.1
│   │   │   ├── ahash v0.8.12 (*)
│   │   │   └── metrics-macros v0.7.1 (proc-macro)
│   │   │       ├── proc-macro2 v1.0.101 (*)
│   │   │       ├── quote v1.0.41 (*)
│   │   │       └── syn v2.0.106 (*)
│   │   ├── mio v0.8.11
│   │   │   ├── libc v0.2.176
│   │   │   └── log v0.4.28 (*)
│   │   ├── once_cell v1.21.3
│   │   ├── pin-project v1.1.10
│   │   │   └── pin-project-internal v1.1.10 (proc-macro)
│   │   │       ├── proc-macro2 v1.0.101 (*)
│   │   │       ├── quote v1.0.41 (*)
│   │   │       └── syn v2.0.106 (*)
│   │   ├── pin-project-lite v0.2.16
│   │   ├── rand v0.8.5 (*)
│   │   ├── sha1 v0.10.6
│   │   │   ├── cfg-if v1.0.3
│   │   │   ├── cpufeatures v0.2.17
│   │   │   └── digest v0.10.7
│   │   │       ├── block-buffer v0.10.4
│   │   │       │   └── generic-array v0.14.7
│   │   │       │       └── typenum v1.18.0
│   │   │       │       [build-dependencies]
│   │   │       │       └── version_check v0.9.5
│   │   │       └── crypto-common v0.1.6
│   │   │           ├── generic-array v0.14.7 (*)
│   │   │           └── typenum v1.18.0
│   │   ├── socket2 v0.5.10
│   │   │   └── libc v0.2.176
│   │   └── tokio v1.47.1
│   │       ├── bytes v1.10.1
│   │       ├── libc v0.2.176
│   │       ├── mio v1.0.4
│   │       │   └── libc v0.2.176
│   │       ├── pin-project-lite v0.2.16
│   │       ├── socket2 v0.6.0
│   │       │   └── libc v0.2.176
│   │       └── tokio-macros v2.5.0 (proc-macro)
│   │           ├── proc-macro2 v1.0.101 (*)
│   │           ├── quote v1.0.41 (*)
│   │           └── syn v2.0.106 (*)
│   ├── serde v1.0.228 (*)
│   ├── serde_json v1.0.145 (*)
│   ├── thiserror v1.0.69 (*)
│   ├── tracing v0.1.41
│   │   ├── log v0.4.28 (*)
│   │   ├── pin-project-lite v0.2.16
│   │   ├── tracing-attributes v0.1.30 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   ├── quote v1.0.41 (*)
│   │   │   └── syn v2.0.106 (*)
│   │   └── tracing-core v0.1.34
│   │       └── once_cell v1.21.3
│   └── url v2.5.7
│       ├── form_urlencoded v1.2.2
│       │   └── percent-encoding v2.3.2
│       ├── idna v1.1.0
│       │   ├── idna_adapter v1.2.1
│       │   │   ├── icu_normalizer v2.0.0
│       │   │   │   ├── displaydoc v0.2.5 (proc-macro)
│       │   │   │   │   ├── proc-macro2 v1.0.101 (*)
│       │   │   │   │   ├── quote v1.0.41 (*)
│       │   │   │   │   └── syn v2.0.106 (*)
│       │   │   │   ├── icu_collections v2.0.0
│       │   │   │   │   ├── displaydoc v0.2.5 (proc-macro) (*)
│       │   │   │   │   ├── potential_utf v0.1.3
│       │   │   │   │   │   └── zerovec v0.11.4
│       │   │   │   │   │       ├── yoke v0.8.0
│       │   │   │   │   │       │   ├── stable_deref_trait v1.2.0
│       │   │   │   │   │       │   ├── yoke-derive v0.8.0 (proc-macro)
│       │   │   │   │   │       │   │   ├── proc-macro2 v1.0.101 (*)
│       │   │   │   │   │       │   │   ├── quote v1.0.41 (*)
│       │   │   │   │   │       │   │   ├── syn v2.0.106 (*)
│       │   │   │   │   │       │   │   └── synstructure v0.13.2
│       │   │   │   │   │       │   │       ├── proc-macro2 v1.0.101 (*)
│       │   │   │   │   │       │   │       ├── quote v1.0.41 (*)
│       │   │   │   │   │       │   │       └── syn v2.0.106 (*)
│       │   │   │   │   │       │   └── zerofrom v0.1.6
│       │   │   │   │   │       │       └── zerofrom-derive v0.1.6 (proc-macro)
│       │   │   │   │   │       │           ├── proc-macro2 v1.0.101 (*)
│       │   │   │   │   │       │           ├── quote v1.0.41 (*)
│       │   │   │   │   │       │           ├── syn v2.0.106 (*)
│       │   │   │   │   │       │           └── synstructure v0.13.2 (*)
│       │   │   │   │   │       ├── zerofrom v0.1.6 (*)
│       │   │   │   │   │       └── zerovec-derive v0.11.1 (proc-macro)
│       │   │   │   │   │           ├── proc-macro2 v1.0.101 (*)
│       │   │   │   │   │           ├── quote v1.0.41 (*)
│       │   │   │   │   │           └── syn v2.0.106 (*)
│       │   │   │   │   ├── yoke v0.8.0 (*)
│       │   │   │   │   ├── zerofrom v0.1.6 (*)
│       │   │   │   │   └── zerovec v0.11.4 (*)
│       │   │   │   ├── icu_normalizer_data v2.0.0
│       │   │   │   ├── icu_provider v2.0.0
│       │   │   │   │   ├── displaydoc v0.2.5 (proc-macro) (*)
│       │   │   │   │   ├── icu_locale_core v2.0.0
│       │   │   │   │   │   ├── displaydoc v0.2.5 (proc-macro) (*)
│       │   │   │   │   │   ├── litemap v0.8.0
│       │   │   │   │   │   ├── tinystr v0.8.1
│       │   │   │   │   │   │   ├── displaydoc v0.2.5 (proc-macro) (*)
│       │   │   │   │   │   │   └── zerovec v0.11.4 (*)
│       │   │   │   │   │   ├── writeable v0.6.1
│       │   │   │   │   │   └── zerovec v0.11.4 (*)
│       │   │   │   │   ├── stable_deref_trait v1.2.0
│       │   │   │   │   ├── tinystr v0.8.1 (*)
│       │   │   │   │   ├── writeable v0.6.1
│       │   │   │   │   ├── yoke v0.8.0 (*)
│       │   │   │   │   ├── zerofrom v0.1.6 (*)
│       │   │   │   │   ├── zerotrie v0.2.2
│       │   │   │   │   │   ├── displaydoc v0.2.5 (proc-macro) (*)
│       │   │   │   │   │   ├── yoke v0.8.0 (*)
│       │   │   │   │   │   └── zerofrom v0.1.6 (*)
│       │   │   │   │   └── zerovec v0.11.4 (*)
│       │   │   │   ├── smallvec v1.15.1 (*)
│       │   │   │   └── zerovec v0.11.4 (*)
│       │   │   └── icu_properties v2.0.1
│       │   │       ├── displaydoc v0.2.5 (proc-macro) (*)
│       │   │       ├── icu_collections v2.0.0 (*)
│       │   │       ├── icu_locale_core v2.0.0 (*)
│       │   │       ├── icu_properties_data v2.0.1
│       │   │       ├── icu_provider v2.0.0 (*)
│       │   │       ├── potential_utf v0.1.3 (*)
│       │   │       ├── zerotrie v0.2.2 (*)
│       │   │       └── zerovec v0.11.4 (*)
│       │   ├── smallvec v1.15.1 (*)
│       │   └── utf8_iter v1.0.4
│       ├── percent-encoding v2.3.2
│       └── serde v1.0.228 (*)
├── indexmap v2.11.4 (*)
├── inflation v0.1.0 (/workspace/the-block/inflation)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── rand v0.8.5 (*)
│   ├── serde v1.0.228 (*)
│   └── serde_json v1.0.145 (*)
├── jsonrpc-core v18.0.0
│   ├── futures v0.3.31 (*)
│   ├── futures-executor v0.3.31 (*)
│   ├── futures-util v0.3.31 (*)
│   ├── log v0.4.28 (*)
│   ├── serde v1.0.228 (*)
│   ├── serde_derive v1.0.228 (proc-macro) (*)
│   └── serde_json v1.0.145 (*)
├── jurisdiction v0.1.0 (/workspace/the-block/crates/jurisdiction)
│   ├── base64 v0.22.1
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── log v0.4.28 (*)
│   ├── once_cell v1.21.3
│   ├── serde v1.0.228 (*)
│   ├── serde_json v1.0.145 (*)
│   └── ureq v2.12.1
│       ├── base64 v0.22.1
│       ├── flate2 v1.1.2 (*)
│       ├── log v0.4.28 (*)
│       ├── once_cell v1.21.3
│       ├── rustls v0.23.32
│       │   ├── log v0.4.28 (*)
│       │   ├── once_cell v1.21.3
│       │   ├── ring v0.17.14
│       │   │   ├── cfg-if v1.0.3
│       │   │   ├── getrandom v0.2.16 (*)
│       │   │   └── untrusted v0.9.0
│       │   │   [build-dependencies]
│       │   │   └── cc v1.2.39 (*)
│       │   ├── rustls-pki-types v1.12.0
│       │   │   └── zeroize v1.8.2
│       │   ├── rustls-webpki v0.103.7
│       │   │   ├── ring v0.17.14 (*)
│       │   │   ├── rustls-pki-types v1.12.0 (*)
│       │   │   └── untrusted v0.9.0
│       │   ├── subtle v2.6.1
│       │   └── zeroize v1.8.2
│       ├── rustls-pki-types v1.12.0 (*)
│       ├── serde v1.0.228 (*)
│       ├── serde_json v1.0.145 (*)
│       ├── url v2.5.7 (*)
│       └── webpki-roots v0.26.11
│           └── webpki-roots v1.0.2
│               └── rustls-pki-types v1.12.0 (*)
├── ledger v0.1.0 (/workspace/the-block/ledger) (*)
├── light-client v0.1.0 (/workspace/the-block/crates/light-client)
│   ├── async-trait v0.1.89 (proc-macro)
│   │   ├── proc-macro2 v1.0.101 (*)
│   │   ├── quote v1.0.41 (*)
│   │   └── syn v2.0.106 (*)
│   ├── bincode v1.3.3 (*)
│   ├── coding v0.1.0 (/workspace/the-block/crates/coding) (*)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── dirs v5.0.1 (*)
│   ├── flate2 v1.1.2 (*)
│   ├── futures v0.3.31 (*)
│   ├── runtime v0.1.0 (/workspace/the-block/crates/runtime) (*)
│   ├── serde v1.0.228 (*)
│   ├── state v0.1.0 (/workspace/the-block/state)
│   │   ├── bincode v1.3.3 (*)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   ├── hex v0.4.3
│   │   ├── rocksdb v0.21.0 (/workspace/the-block/vendor/rocksdb-0.21.0/rocksdb-0.21.0) (*)
│   │   ├── serde v1.0.228 (*)
│   │   ├── serde_json v1.0.145 (*)
│   │   └── thiserror v1.0.69 (*)
│   ├── thiserror v1.0.69 (*)
│   ├── tokio v1.47.1 (*)
│   ├── toml v0.8.23 (*)
│   └── tracing v0.1.41 (*)
├── lru v0.11.1
│   └── hashbrown v0.14.5 (*)
├── nalgebra v0.32.6 (*)
├── nix v0.27.1
│   ├── bitflags v2.9.4
│   ├── cfg-if v1.0.3
│   └── libc v0.2.176
├── notify v6.1.1
│   ├── crossbeam-channel v0.5.15 (*)
│   ├── filetime v0.2.26
│   │   ├── cfg-if v1.0.3
│   │   └── libc v0.2.176
│   ├── inotify v0.9.6
│   │   ├── bitflags v1.3.2
│   │   ├── inotify-sys v0.1.5
│   │   │   └── libc v0.2.176
│   │   └── libc v0.2.176
│   ├── libc v0.2.176
│   ├── log v0.4.28 (*)
│   ├── mio v0.8.11 (*)
│   └── walkdir v2.5.0
│       └── same-file v1.0.6
├── num_cpus v1.17.0
│   └── libc v0.2.176
├── once_cell v1.21.3
├── p2p_overlay v0.1.0 (/workspace/the-block/crates/p2p_overlay)
│   ├── bs58 v0.4.0
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── serde v1.0.228 (*)
│   ├── serde_json v1.0.145 (*)
│   └── thiserror v1.0.69 (*)
├── parking_lot v0.12.4 (*)
├── pprof v0.13.0
│   ├── backtrace v0.3.76 (*)
│   ├── cfg-if v1.0.3
│   ├── findshlibs v0.10.2
│   │   └── libc v0.2.176
│   │   [build-dependencies]
│   │   └── cc v1.2.39 (*)
│   ├── inferno v0.11.21
│   │   ├── ahash v0.8.12 (*)
│   │   ├── indexmap v2.11.4 (*)
│   │   ├── is-terminal v0.4.16
│   │   │   └── libc v0.2.176
│   │   ├── itoa v1.0.15
│   │   ├── log v0.4.28 (*)
│   │   ├── num-format v0.4.4
│   │   │   ├── arrayvec v0.7.6
│   │   │   └── itoa v1.0.15
│   │   ├── once_cell v1.21.3
│   │   ├── quick-xml v0.26.0
│   │   │   └── memchr v2.7.6
│   │   ├── rgb v0.8.52
│   │   │   └── bytemuck v1.23.2
│   │   └── str_stack v0.1.0
│   ├── libc v0.2.176
│   ├── log v0.4.28 (*)
│   ├── nix v0.26.4
│   │   ├── bitflags v1.3.2
│   │   ├── cfg-if v1.0.3
│   │   └── libc v0.2.176
│   ├── once_cell v1.21.3
│   ├── parking_lot v0.12.4 (*)
│   ├── smallvec v1.15.1 (*)
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
├── procfs v0.15.1
│   ├── bitflags v1.3.2
│   ├── byteorder v1.5.0
│   ├── chrono v0.4.42
│   │   ├── iana-time-zone v0.1.64
│   │   └── num-traits v0.2.19 (*)
│   ├── flate2 v1.1.2 (*)
│   ├── hex v0.4.3
│   ├── lazy_static v1.5.0
│   └── rustix v0.36.17
│       ├── bitflags v1.3.2
│       ├── io-lifetimes v1.0.11
│       │   └── libc v0.2.176
│       ├── libc v0.2.176
│       └── linux-raw-sys v0.1.4
├── pyo3 v0.24.2
│   ├── cfg-if v1.0.3
│   ├── indoc v2.0.6 (proc-macro)
│   ├── libc v0.2.176
│   ├── memoffset v0.9.1
│   │   [build-dependencies]
│   │   └── autocfg v1.5.0
│   ├── once_cell v1.21.3
│   ├── pyo3-ffi v0.24.2
│   │   └── libc v0.2.176
│   │   [build-dependencies]
│   │   └── pyo3-build-config v0.24.2
│   │       ├── once_cell v1.21.3
│   │       └── target-lexicon v0.13.3
│   │       [build-dependencies]
│   │       └── target-lexicon v0.13.3
│   ├── pyo3-macros v0.24.2 (proc-macro)
│   │   ├── proc-macro2 v1.0.101 (*)
│   │   ├── pyo3-macros-backend v0.24.2
│   │   │   ├── heck v0.5.0
│   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   ├── pyo3-build-config v0.24.2 (*)
│   │   │   ├── quote v1.0.41 (*)
│   │   │   └── syn v2.0.106 (*)
│   │   │   [build-dependencies]
│   │   │   └── pyo3-build-config v0.24.2 (*)
│   │   ├── quote v1.0.41 (*)
│   │   └── syn v2.0.106 (*)
│   └── unindent v0.2.4
│   [build-dependencies]
│   └── pyo3-build-config v0.24.2 (*)
├── rand v0.8.5 (*)
├── rand_core v0.6.4 (*)
├── rayon v1.11.0
│   ├── either v1.15.0
│   └── rayon-core v1.13.0
│       ├── crossbeam-deque v0.8.6 (*)
│       └── crossbeam-utils v0.8.21
├── regex v1.11.3
│   ├── aho-corasick v1.1.3
│   │   └── memchr v2.7.6
│   ├── memchr v2.7.6
│   ├── regex-automata v0.4.11
│   │   ├── aho-corasick v1.1.3 (*)
│   │   ├── memchr v2.7.6
│   │   └── regex-syntax v0.8.6
│   └── regex-syntax v0.8.6
├── ripemd v0.1.3
│   └── digest v0.10.7 (*)
├── rocksdb v0.21.0 (/workspace/the-block/vendor/rocksdb-0.21.0/rocksdb-0.21.0) (*)
├── runtime v0.1.0 (/workspace/the-block/crates/runtime) (*)
├── rusqlite v0.30.0
│   ├── bitflags v2.9.4
│   ├── fallible-iterator v0.3.0
│   ├── fallible-streaming-iterator v0.1.9
│   ├── hashlink v0.8.4
│   │   └── hashbrown v0.14.5 (*)
│   ├── libsqlite3-sys v0.27.0
│   │   [build-dependencies]
│   │   ├── cc v1.2.39 (*)
│   │   ├── pkg-config v0.3.32
│   │   └── vcpkg v0.2.15
│   └── smallvec v1.15.1 (*)
├── rustdct v0.7.1 (*)
├── serde v1.0.228 (*)
├── serde_bytes v0.11.19
│   └── serde_core v1.0.228
├── serde_cbor v0.11.2 (*)
├── serde_json v1.0.145 (*)
├── sha1 v0.10.6 (*)
├── signal-hook v0.3.18
│   ├── libc v0.2.176
│   └── signal-hook-registry v1.4.6
│       └── libc v0.2.176
├── sled v0.34.7 (*)
├── state v0.1.0 (/workspace/the-block/state) (*)
├── static_assertions v1.1.0
├── statrs v0.16.1 (*)
├── storage v0.1.0 (/workspace/the-block/storage)
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── serde v1.0.228 (*)
│   └── thiserror v1.0.69 (*)
├── storage_engine v0.1.0 (/workspace/the-block/crates/storage_engine) (*)
├── subtle v2.6.1
├── tar v0.4.44
│   ├── filetime v0.2.26 (*)
│   ├── libc v0.2.176
│   └── xattr v1.6.1
│       └── rustix v1.1.2 (*)
├── tempfile v3.23.0 (*)
├── terminal_size v0.2.6
│   └── rustix v0.37.28
│       ├── bitflags v1.3.2
│       ├── io-lifetimes v1.0.11 (*)
│       ├── libc v0.2.176
│       └── linux-raw-sys v0.3.8
├── thiserror v1.0.69 (*)
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
├── tokio v1.47.1 (*)
├── tokio-util v0.7.16
│   ├── bytes v1.10.1
│   ├── futures-core v0.3.31
│   ├── futures-sink v0.3.31
│   ├── pin-project-lite v0.2.16
│   └── tokio v1.47.1 (*)
├── toml v0.8.23 (*)
├── tracing-chrome v0.6.0
│   ├── crossbeam v0.8.4
│   │   ├── crossbeam-channel v0.5.15 (*)
│   │   ├── crossbeam-deque v0.8.6 (*)
│   │   ├── crossbeam-epoch v0.9.18 (*)
│   │   ├── crossbeam-queue v0.3.12
│   │   │   └── crossbeam-utils v0.8.21
│   │   └── crossbeam-utils v0.8.21
│   ├── json v0.12.4
│   ├── tracing v0.1.41 (*)
│   └── tracing-subscriber v0.3.20
│       ├── matchers v0.2.0
│       │   └── regex-automata v0.4.11 (*)
│       ├── nu-ansi-term v0.50.1
│       ├── once_cell v1.21.3
│       ├── regex-automata v0.4.11 (*)
│       ├── serde v1.0.228 (*)
│       ├── serde_json v1.0.145 (*)
│       ├── sharded-slab v0.1.7
│       │   └── lazy_static v1.5.0
│       ├── smallvec v1.15.1 (*)
│       ├── thread_local v1.1.9
│       │   └── cfg-if v1.0.3
│       ├── tracing v0.1.41 (*)
│       ├── tracing-core v0.1.34 (*)
│       ├── tracing-log v0.2.0
│       │   ├── log v0.4.28 (*)
│       │   ├── once_cell v1.21.3
│       │   └── tracing-core v0.1.34 (*)
│       └── tracing-serde v0.2.0
│           ├── serde v1.0.228 (*)
│           └── tracing-core v0.1.34 (*)
├── tracing-subscriber v0.3.20 (*)
├── trust-dns-resolver v0.23.2
│   ├── cfg-if v1.0.3
│   ├── futures-util v0.3.31 (*)
│   ├── lru-cache v0.1.2
│   │   └── linked-hash-map v0.5.6
│   ├── once_cell v1.21.3
│   ├── parking_lot v0.12.4 (*)
│   ├── rand v0.8.5 (*)
│   ├── resolv-conf v0.7.5
│   ├── smallvec v1.15.1 (*)
│   ├── thiserror v1.0.69 (*)
│   ├── tokio v1.47.1 (*)
│   ├── tracing v0.1.41 (*)
│   └── trust-dns-proto v0.23.2
│       ├── async-trait v0.1.89 (proc-macro) (*)
│       ├── cfg-if v1.0.3
│       ├── data-encoding v2.9.0
│       ├── enum-as-inner v0.6.1 (proc-macro)
│       │   ├── heck v0.5.0
│       │   ├── proc-macro2 v1.0.101 (*)
│       │   ├── quote v1.0.41 (*)
│       │   └── syn v2.0.106 (*)
│       ├── futures-channel v0.3.31 (*)
│       ├── futures-io v0.3.31
│       ├── futures-util v0.3.31 (*)
│       ├── idna v0.4.0
│       │   ├── unicode-bidi v0.3.18
│       │   └── unicode-normalization v0.1.24
│       │       └── tinyvec v1.10.0
│       │           └── tinyvec_macros v0.1.1
│       ├── ipnet v2.11.0
│       ├── once_cell v1.21.3
│       ├── rand v0.8.5 (*)
│       ├── smallvec v1.15.1 (*)
│       ├── thiserror v1.0.69 (*)
│       ├── tinyvec v1.10.0 (*)
│       ├── tokio v1.47.1 (*)
│       ├── tracing v0.1.41 (*)
│       └── url v2.5.7 (*)
├── unicode-normalization v0.1.24 (*)
├── url v2.5.7 (*)
├── wallet v0.1.0 (/workspace/the-block/crates/wallet)
│   ├── base64 v0.22.1
│   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   ├── hex v0.4.3
│   ├── httpd v0.1.0 (/workspace/the-block/crates/httpd) (*)
│   ├── ledger v0.1.0 (/workspace/the-block/ledger) (*)
│   ├── metrics v0.21.1 (*)
│   ├── native-tls v0.2.14
│   │   ├── log v0.4.28 (*)
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
│   │   │       ├── cc v1.2.39 (*)
│   │   │       ├── pkg-config v0.3.32
│   │   │       └── vcpkg v0.2.15
│   │   ├── openssl-probe v0.1.6
│   │   └── openssl-sys v0.9.109 (*)
│   ├── once_cell v1.21.3
│   ├── rand v0.8.5 (*)
│   ├── serde v1.0.228 (*)
│   ├── serde_json v1.0.145 (*)
│   ├── sha1 v0.10.6 (*)
│   ├── subtle v2.6.1
│   ├── thiserror v1.0.69 (*)
│   ├── tracing v0.1.41 (*)
│   ├── url v2.5.7 (*)
│   └── uuid v1.18.1 (*)
├── wasmtime v24.0.4
│   ├── addr2line v0.22.0
│   │   └── gimli v0.29.0
│   │       └── indexmap v2.11.4 (*)
│   ├── anyhow v1.0.100
│   ├── async-trait v0.1.89 (proc-macro) (*)
│   ├── bitflags v2.9.4
│   ├── bumpalo v3.19.0
│   ├── cfg-if v1.0.3
│   ├── encoding_rs v0.8.35
│   │   └── cfg-if v1.0.3
│   ├── fxprof-processed-profile v0.6.0
│   │   ├── bitflags v2.9.4
│   │   ├── debugid v0.8.0 (*)
│   │   ├── fxhash v0.2.1 (*)
│   │   ├── serde v1.0.228 (*)
│   │   └── serde_json v1.0.145 (*)
│   ├── gimli v0.29.0 (*)
│   ├── hashbrown v0.14.5 (*)
│   ├── indexmap v2.11.4 (*)
│   ├── ittapi v0.4.0
│   │   ├── anyhow v1.0.100
│   │   ├── ittapi-sys v0.4.0
│   │   │   [build-dependencies]
│   │   │   └── cc v1.2.39 (*)
│   │   └── log v0.4.28 (*)
│   ├── libc v0.2.176
│   ├── libm v0.2.15
│   ├── log v0.4.28 (*)
│   ├── memfd v0.6.5
│   │   └── rustix v1.1.2 (*)
│   ├── object v0.36.7
│   │   ├── crc32fast v1.5.0 (*)
│   │   ├── hashbrown v0.15.5
│   │   │   └── foldhash v0.1.5
│   │   ├── indexmap v2.11.4 (*)
│   │   └── memchr v2.7.6
│   ├── once_cell v1.21.3
│   ├── paste v1.0.15 (proc-macro)
│   ├── postcard v1.1.3
│   │   ├── cobs v0.3.0
│   │   │   └── thiserror v2.0.17
│   │   │       └── thiserror-impl v2.0.17 (proc-macro)
│   │   │           ├── proc-macro2 v1.0.101 (*)
│   │   │           ├── quote v1.0.41 (*)
│   │   │           └── syn v2.0.106 (*)
│   │   └── serde v1.0.228 (*)
│   ├── rayon v1.11.0 (*)
│   ├── rustix v0.38.44
│   │   ├── bitflags v2.9.4
│   │   └── linux-raw-sys v0.4.15
│   ├── semver v1.0.27
│   │   └── serde_core v1.0.228
│   ├── serde v1.0.228 (*)
│   ├── serde_derive v1.0.228 (proc-macro) (*)
│   ├── serde_json v1.0.145 (*)
│   ├── smallvec v1.15.1 (*)
│   ├── sptr v0.3.2
│   ├── target-lexicon v0.12.16
│   ├── wasm-encoder v0.215.0
│   │   └── leb128 v0.2.5
│   ├── wasmparser v0.215.0
│   │   ├── ahash v0.8.12 (*)
│   │   ├── bitflags v2.9.4
│   │   ├── hashbrown v0.14.5 (*)
│   │   ├── indexmap v2.11.4 (*)
│   │   ├── semver v1.0.27 (*)
│   │   └── serde v1.0.228 (*)
│   ├── wasmtime-asm-macros v24.0.4
│   │   └── cfg-if v1.0.3
│   ├── wasmtime-cache v24.0.4
│   │   ├── anyhow v1.0.100
│   │   ├── base64 v0.21.7
│   │   ├── directories-next v2.0.0
│   │   │   ├── cfg-if v1.0.3
│   │   │   └── dirs-sys-next v0.1.2
│   │   │       └── libc v0.2.176
│   │   ├── log v0.4.28 (*)
│   │   ├── postcard v1.1.3 (*)
│   │   ├── rustix v0.38.44 (*)
│   │   ├── serde v1.0.228 (*)
│   │   ├── serde_derive v1.0.228 (proc-macro) (*)
│   │   ├── sha2 v0.10.9
│   │   │   ├── cfg-if v1.0.3
│   │   │   ├── cpufeatures v0.2.17
│   │   │   └── digest v0.10.7 (*)
│   │   ├── toml v0.8.23 (*)
│   │   └── zstd v0.13.3
│   │       └── zstd-safe v7.2.4
│   │           └── zstd-sys v2.0.16+zstd.1.5.7 (*)
│   ├── wasmtime-component-macro v24.0.4 (proc-macro)
│   │   ├── anyhow v1.0.100
│   │   ├── proc-macro2 v1.0.101 (*)
│   │   ├── quote v1.0.41 (*)
│   │   ├── syn v2.0.106 (*)
│   │   ├── wasmtime-component-util v24.0.4
│   │   ├── wasmtime-wit-bindgen v24.0.4
│   │   │   ├── anyhow v1.0.100
│   │   │   ├── heck v0.4.1
│   │   │   ├── indexmap v2.11.4
│   │   │   │   ├── equivalent v1.0.2
│   │   │   │   ├── hashbrown v0.16.0
│   │   │   │   └── serde_core v1.0.228
│   │   │   └── wit-parser v0.215.0
│   │   │       ├── anyhow v1.0.100
│   │   │       ├── id-arena v2.2.1
│   │   │       ├── indexmap v2.11.4 (*)
│   │   │       ├── log v0.4.28
│   │   │       ├── semver v1.0.27
│   │   │       ├── serde v1.0.228
│   │   │       │   └── serde_core v1.0.228
│   │   │       ├── serde_derive v1.0.228 (proc-macro) (*)
│   │   │       ├── serde_json v1.0.145
│   │   │       │   ├── itoa v1.0.15
│   │   │       │   ├── memchr v2.7.6
│   │   │       │   ├── ryu v1.0.20
│   │   │       │   └── serde_core v1.0.228
│   │   │       ├── unicode-xid v0.2.6
│   │   │       └── wasmparser v0.215.0
│   │   │           ├── ahash v0.8.12
│   │   │           │   ├── cfg-if v1.0.3
│   │   │           │   ├── once_cell v1.21.3
│   │   │           │   └── zerocopy v0.8.27
│   │   │           │   [build-dependencies]
│   │   │           │   └── version_check v0.9.5
│   │   │           ├── bitflags v2.9.4
│   │   │           ├── hashbrown v0.14.5
│   │   │           │   └── ahash v0.8.12 (*)
│   │   │           ├── indexmap v2.11.4 (*)
│   │   │           └── semver v1.0.27
│   │   └── wit-parser v0.215.0 (*)
│   ├── wasmtime-component-util v24.0.4
│   ├── wasmtime-cranelift v24.0.4
│   │   ├── anyhow v1.0.100
│   │   ├── cfg-if v1.0.3
│   │   ├── cranelift-codegen v0.111.4
│   │   │   ├── bumpalo v3.19.0
│   │   │   ├── cranelift-bforest v0.111.4
│   │   │   │   └── cranelift-entity v0.111.4
│   │   │   │       ├── cranelift-bitset v0.111.4
│   │   │   │       │   ├── serde v1.0.228 (*)
│   │   │   │       │   └── serde_derive v1.0.228 (proc-macro) (*)
│   │   │   │       ├── serde v1.0.228 (*)
│   │   │   │       └── serde_derive v1.0.228 (proc-macro) (*)
│   │   │   ├── cranelift-bitset v0.111.4 (*)
│   │   │   ├── cranelift-codegen-shared v0.111.4
│   │   │   ├── cranelift-control v0.111.4
│   │   │   │   └── arbitrary v1.4.2
│   │   │   │       └── derive_arbitrary v1.4.2 (proc-macro)
│   │   │   │           ├── proc-macro2 v1.0.101 (*)
│   │   │   │           ├── quote v1.0.41 (*)
│   │   │   │           └── syn v2.0.106 (*)
│   │   │   ├── cranelift-entity v0.111.4 (*)
│   │   │   ├── gimli v0.29.0 (*)
│   │   │   ├── hashbrown v0.14.5 (*)
│   │   │   ├── log v0.4.28 (*)
│   │   │   ├── regalloc2 v0.9.3
│   │   │   │   ├── hashbrown v0.13.2
│   │   │   │   │   └── ahash v0.8.12 (*)
│   │   │   │   ├── log v0.4.28 (*)
│   │   │   │   ├── rustc-hash v1.1.0
│   │   │   │   ├── slice-group-by v0.3.1
│   │   │   │   └── smallvec v1.15.1 (*)
│   │   │   ├── rustc-hash v1.1.0
│   │   │   ├── smallvec v1.15.1 (*)
│   │   │   └── target-lexicon v0.12.16
│   │   │   [build-dependencies]
│   │   │   ├── cranelift-codegen-meta v0.111.4
│   │   │   │   └── cranelift-codegen-shared v0.111.4
│   │   │   └── cranelift-isle v0.111.4
│   │   ├── cranelift-control v0.111.4 (*)
│   │   ├── cranelift-entity v0.111.4 (*)
│   │   ├── cranelift-frontend v0.111.4
│   │   │   ├── cranelift-codegen v0.111.4 (*)
│   │   │   ├── log v0.4.28 (*)
│   │   │   ├── smallvec v1.15.1 (*)
│   │   │   └── target-lexicon v0.12.16
│   │   ├── cranelift-native v0.111.4
│   │   │   ├── cranelift-codegen v0.111.4 (*)
│   │   │   └── target-lexicon v0.12.16
│   │   ├── cranelift-wasm v0.111.4
│   │   │   ├── cranelift-codegen v0.111.4 (*)
│   │   │   ├── cranelift-entity v0.111.4 (*)
│   │   │   ├── cranelift-frontend v0.111.4 (*)
│   │   │   ├── itertools v0.12.1
│   │   │   │   └── either v1.15.0
│   │   │   ├── log v0.4.28 (*)
│   │   │   ├── smallvec v1.15.1 (*)
│   │   │   ├── wasmparser v0.215.0 (*)
│   │   │   └── wasmtime-types v24.0.4
│   │   │       ├── anyhow v1.0.100
│   │   │       ├── cranelift-entity v0.111.4 (*)
│   │   │       ├── serde v1.0.228 (*)
│   │   │       ├── serde_derive v1.0.228 (proc-macro) (*)
│   │   │       ├── smallvec v1.15.1 (*)
│   │   │       └── wasmparser v0.215.0 (*)
│   │   ├── gimli v0.29.0 (*)
│   │   ├── log v0.4.28 (*)
│   │   ├── object v0.36.7 (*)
│   │   ├── target-lexicon v0.12.16
│   │   ├── thiserror v1.0.69 (*)
│   │   ├── wasmparser v0.215.0 (*)
│   │   ├── wasmtime-environ v24.0.4
│   │   │   ├── anyhow v1.0.100
│   │   │   ├── cpp_demangle v0.4.5 (*)
│   │   │   ├── cranelift-bitset v0.111.4 (*)
│   │   │   ├── cranelift-entity v0.111.4 (*)
│   │   │   ├── gimli v0.29.0 (*)
│   │   │   ├── indexmap v2.11.4 (*)
│   │   │   ├── log v0.4.28 (*)
│   │   │   ├── object v0.36.7 (*)
│   │   │   ├── postcard v1.1.3 (*)
│   │   │   ├── rustc-demangle v0.1.26
│   │   │   ├── semver v1.0.27 (*)
│   │   │   ├── serde v1.0.228 (*)
│   │   │   ├── serde_derive v1.0.228 (proc-macro) (*)
│   │   │   ├── target-lexicon v0.12.16
│   │   │   ├── wasm-encoder v0.215.0 (*)
│   │   │   ├── wasmparser v0.215.0 (*)
│   │   │   ├── wasmprinter v0.215.0
│   │   │   │   ├── anyhow v1.0.100
│   │   │   │   ├── termcolor v1.4.1
│   │   │   │   └── wasmparser v0.215.0 (*)
│   │   │   ├── wasmtime-component-util v24.0.4
│   │   │   └── wasmtime-types v24.0.4 (*)
│   │   └── wasmtime-versioned-export-macros v24.0.4 (proc-macro)
│   │       ├── proc-macro2 v1.0.101 (*)
│   │       ├── quote v1.0.41 (*)
│   │       └── syn v2.0.106 (*)
│   ├── wasmtime-environ v24.0.4 (*)
│   ├── wasmtime-fiber v24.0.4
│   │   ├── anyhow v1.0.100
│   │   ├── cfg-if v1.0.3
│   │   ├── rustix v0.38.44 (*)
│   │   ├── wasmtime-asm-macros v24.0.4 (*)
│   │   └── wasmtime-versioned-export-macros v24.0.4 (proc-macro) (*)
│   │   [build-dependencies]
│   │   ├── cc v1.2.39 (*)
│   │   └── wasmtime-versioned-export-macros v24.0.4 (proc-macro) (*)
│   ├── wasmtime-jit-debug v24.0.4
│   │   ├── object v0.36.7 (*)
│   │   ├── once_cell v1.21.3
│   │   ├── rustix v0.38.44 (*)
│   │   └── wasmtime-versioned-export-macros v24.0.4 (proc-macro) (*)
│   ├── wasmtime-jit-icache-coherence v24.0.4
│   │   ├── anyhow v1.0.100
│   │   ├── cfg-if v1.0.3
│   │   └── libc v0.2.176
│   ├── wasmtime-slab v24.0.4
│   ├── wasmtime-versioned-export-macros v24.0.4 (proc-macro) (*)
│   └── wat v1.239.0
│       └── wast v239.0.0
│           ├── bumpalo v3.19.0
│           ├── leb128fmt v0.1.0
│           ├── memchr v2.7.6
│           ├── unicode-width v0.2.1
│           └── wasm-encoder v0.239.0
│               └── leb128fmt v0.1.0
│   [build-dependencies]
│   ├── cc v1.2.39 (*)
│   └── wasmtime-versioned-export-macros v24.0.4 (proc-macro) (*)
├── x509-parser v0.16.0
│   ├── asn1-rs v0.6.2
│   │   ├── asn1-rs-derive v0.5.1 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   ├── quote v1.0.41 (*)
│   │   │   ├── syn v2.0.106 (*)
│   │   │   └── synstructure v0.13.2 (*)
│   │   ├── asn1-rs-impl v0.2.0 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   ├── quote v1.0.41 (*)
│   │   │   └── syn v2.0.106 (*)
│   │   ├── displaydoc v0.2.5 (proc-macro) (*)
│   │   ├── nom v7.1.3 (*)
│   │   ├── num-traits v0.2.19 (*)
│   │   ├── rusticata-macros v4.1.0
│   │   │   └── nom v7.1.3 (*)
│   │   ├── thiserror v1.0.69 (*)
│   │   └── time v0.3.44 (*)
│   ├── data-encoding v2.9.0
│   ├── der-parser v9.0.0
│   │   ├── asn1-rs v0.6.2 (*)
│   │   ├── displaydoc v0.2.5 (proc-macro) (*)
│   │   ├── nom v7.1.3 (*)
│   │   ├── num-bigint v0.4.6 (*)
│   │   ├── num-traits v0.2.19 (*)
│   │   └── rusticata-macros v4.1.0 (*)
│   ├── lazy_static v1.5.0
│   ├── nom v7.1.3 (*)
│   ├── oid-registry v0.7.1
│   │   └── asn1-rs v0.6.2 (*)
│   ├── rusticata-macros v4.1.0 (*)
│   ├── thiserror v1.0.69 (*)
│   └── time v0.3.44 (*)
└── xorfilter-rs v0.5.1
[dev-dependencies]
├── arbitrary v1.4.2 (*)
├── criterion v0.5.1
│   ├── anes v0.1.6
│   ├── cast v0.3.0
│   ├── ciborium v0.2.2
│   │   ├── ciborium-io v0.2.2
│   │   ├── ciborium-ll v0.2.2
│   │   │   ├── ciborium-io v0.2.2
│   │   │   └── half v2.6.0
│   │   │       └── cfg-if v1.0.3
│   │   └── serde v1.0.228 (*)
│   ├── clap v4.5.48 (*)
│   ├── criterion-plot v0.5.0
│   │   ├── cast v0.3.0
│   │   └── itertools v0.10.5
│   │       └── either v1.15.0
│   ├── is-terminal v0.4.16 (*)
│   ├── itertools v0.10.5 (*)
│   ├── num-traits v0.2.19 (*)
│   ├── once_cell v1.21.3
│   ├── oorandom v11.1.5
│   ├── plotters v0.3.7
│   │   ├── num-traits v0.2.19 (*)
│   │   ├── plotters-backend v0.3.7
│   │   └── plotters-svg v0.3.7
│   │       └── plotters-backend v0.3.7
│   ├── rayon v1.11.0 (*)
│   ├── regex v1.11.3 (*)
│   ├── serde v1.0.228 (*)
│   ├── serde_derive v1.0.228 (proc-macro) (*)
│   ├── serde_json v1.0.145 (*)
│   ├── tinytemplate v1.2.1
│   │   ├── serde v1.0.228 (*)
│   │   └── serde_json v1.0.145 (*)
│   └── walkdir v2.5.0 (*)
├── csv v1.3.1
│   ├── csv-core v0.1.12
│   │   └── memchr v2.7.6
│   ├── itoa v1.0.15
│   ├── ryu v1.0.20
│   └── serde v1.0.228 (*)
├── env_logger v0.11.8
│   ├── anstream v0.6.20 (*)
│   ├── anstyle v1.0.13
│   ├── env_filter v0.1.3
│   │   ├── log v0.4.28 (*)
│   │   └── regex v1.11.3 (*)
│   ├── jiff v0.2.15
│   └── log v0.4.28 (*)
├── insta v1.43.2
│   ├── console v0.15.11
│   │   ├── libc v0.2.176
│   │   └── once_cell v1.21.3
│   ├── globset v0.4.16
│   │   ├── aho-corasick v1.1.3 (*)
│   │   ├── bstr v1.12.0
│   │   │   └── memchr v2.7.6
│   │   ├── log v0.4.28 (*)
│   │   ├── regex-automata v0.4.11 (*)
│   │   └── regex-syntax v0.8.6
│   ├── once_cell v1.21.3
│   ├── similar v2.7.0
│   └── walkdir v2.5.0 (*)
├── jurisdiction v0.1.0 (/workspace/the-block/crates/jurisdiction) (*)
├── logtest v2.0.0
│   ├── lazy_static v1.5.0
│   └── log v0.4.28 (*)
├── proptest v1.8.0
│   ├── bit-set v0.8.0
│   │   └── bit-vec v0.8.0
│   ├── bit-vec v0.8.0
│   ├── bitflags v2.9.4
│   ├── lazy_static v1.5.0
│   ├── num-traits v0.2.19 (*)
│   ├── rand v0.9.2
│   │   └── rand_core v0.9.3
│   │       └── getrandom v0.3.3 (*)
│   ├── rand_chacha v0.9.0
│   │   ├── ppv-lite86 v0.2.21 (*)
│   │   └── rand_core v0.9.3 (*)
│   ├── rand_xorshift v0.4.0
│   │   └── rand_core v0.9.3 (*)
│   ├── regex-syntax v0.8.6
│   ├── rusty-fork v0.3.0
│   │   ├── fnv v1.0.7
│   │   ├── quick-error v1.2.3
│   │   ├── tempfile v3.23.0 (*)
│   │   └── wait-timeout v0.2.1
│   │       └── libc v0.2.176
│   ├── tempfile v3.23.0 (*)
│   └── unarray v0.1.4
├── serial_test v3.2.0
│   ├── futures v0.3.31 (*)
│   ├── log v0.4.28 (*)
│   ├── once_cell v1.21.3
│   ├── parking_lot v0.12.4 (*)
│   ├── scc v2.4.0
│   │   └── sdd v3.0.10
│   └── serial_test_derive v3.2.0 (proc-macro)
│       ├── proc-macro2 v1.0.101 (*)
│       ├── quote v1.0.41 (*)
│       └── syn v2.0.106 (*)
├── tar v0.4.44 (*)
├── tb-sim v0.1.0 (/workspace/the-block/sim)
│   ├── anyhow v1.0.100
│   ├── bincode v1.3.3 (*)
│   ├── clap v4.5.48 (*)
│   ├── csv v1.3.1 (*)
│   ├── dex v0.1.0 (/workspace/the-block/dex) (*)
│   ├── dkg v0.1.0 (/workspace/the-block/dkg) (*)
│   ├── explorer v0.1.0 (/workspace/the-block/explorer)
│   │   ├── anyhow v1.0.100
│   │   ├── axum v0.7.9 (*)
│   │   ├── bincode v1.3.3 (*)
│   │   ├── codec v0.1.0 (/workspace/the-block/crates/codec) (*)
│   │   ├── crypto_suite v0.1.0 (/workspace/the-block/crates/crypto_suite) (*)
│   │   ├── hex v0.4.3
│   │   ├── lru v0.11.1 (*)
│   │   ├── rusqlite v0.30.0 (*)
│   │   ├── serde v1.0.228 (*)
│   │   ├── serde_json v1.0.145 (*)
│   │   ├── storage v0.1.0 (/workspace/the-block/storage) (*)
│   │   ├── the_block v0.1.0 (/workspace/the-block/node) (*)
│   │   ├── tokio v1.47.1 (*)
│   │   └── wasmprinter v0.2.80
│   │       ├── anyhow v1.0.100
│   │       └── wasmparser v0.121.2
│   │           ├── bitflags v2.9.4
│   │           ├── indexmap v2.11.4 (*)
│   │           └── semver v1.0.27 (*)
│   ├── hex v0.4.3
│   ├── ledger v0.1.0 (/workspace/the-block/ledger) (*)
│   ├── light-client v0.1.0 (/workspace/the-block/crates/light-client) (*)
│   ├── once_cell v1.21.3
│   ├── rand v0.8.5 (*)
│   ├── rocksdb v0.21.0 (/workspace/the-block/vendor/rocksdb-0.21.0/rocksdb-0.21.0) (*)
│   ├── serde v1.0.228 (*)
│   ├── serde_json v1.0.145 (*)
│   ├── tempfile v3.23.0 (*)
│   ├── the_block v0.1.0 (/workspace/the-block/node) (*)
│   ├── thiserror v1.0.69 (*)
│   └── tokio-util v0.7.16 (*)
├── tracing v0.1.41 (*)
├── tracing-test v0.2.5
│   ├── tracing-core v0.1.34 (*)
│   ├── tracing-subscriber v0.3.20 (*)
│   └── tracing-test-macro v0.2.5 (proc-macro)
│       ├── quote v1.0.41 (*)
│       └── syn v2.0.106 (*)
├── wait-timeout v0.2.1 (*)
└── wat v1.239.0 (*)
```
