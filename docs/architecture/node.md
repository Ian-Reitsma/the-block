# Node Dependency Tree
> **Review (2025-09-25):** Synced Node Dependency Tree guidance with the dependency-sovereignty pivot and confirmed readiness + token hygiene.
> Dependency pivot status: Runtime, transport, overlay, storage_engine, coding, crypto_suite, and codec wrappers are live with governance overrides enforced (2025-09-25). Runtime-native WebSockets (`runtime::ws`) now back `/logs/tail`, `/state/stream`, `/vm.trace`, the gateway peer-metrics feed, and CLI consumers, eliminating the `tokio-tungstenite`/`hyper-tungstenite` stack across the workspace (2025-10-02).

This document lists the dependency hierarchy for the `the_block` node crate. It is generated via `cargo tree --manifest-path node/Cargo.toml`.

> **Update (2025-11-05):** The workspace now relies on the first-party
> `httpd` crate for outbound HTTP/JSON traffic. The tables below still show
> legacy `reqwest` entries from the previous capture; treat them as historical
> until the next tree export lands.

```
the_block v0.1.0 (/workspace/the-block/node)
├── anyhow v1.0.99
├── base64 v0.22.1
├── base64ct v1.8.0
├── bellman_ce v0.8.0
│   ├── arrayvec v0.7.6
│   ├── bit-vec v0.6.3
│   │   └── serde v1.0.219
│   │       └── serde_derive v1.0.219 (proc-macro)
│   │           ├── proc-macro2 v1.0.101
│   │           │   └── unicode-ident v1.0.19
│   │           ├── quote v1.0.40
│   │           │   └── proc-macro2 v1.0.101 (*)
│   │           └── syn v2.0.106
│   │               ├── proc-macro2 v1.0.101 (*)
│   │               ├── quote v1.0.40 (*)
│   │               └── unicode-ident v1.0.19
│   ├── blake2s_const v0.8.0
│   │   ├── arrayref v0.3.9
│   │   ├── arrayvec v0.5.2
│   │   └── constant_time_eq v0.1.5
│   ├── blake2s_simd v0.5.11
│   │   ├── arrayref v0.3.9
│   │   ├── arrayvec v0.5.2
│   │   └── constant_time_eq v0.1.5
│   ├── byteorder v1.5.0
│   ├── cfg-if v1.0.3
│   ├── crossbeam v0.7.3
│   │   ├── cfg-if v0.1.10
│   │   ├── crossbeam-channel v0.4.4
│   │   │   ├── crossbeam-utils v0.7.2
│   │   │   │   ├── cfg-if v0.1.10
│   │   │   │   └── lazy_static v1.5.0
│   │   │   │   [build-dependencies]
│   │   │   │   └── autocfg v1.5.0
│   │   │   └── maybe-uninit v2.0.0
│   │   ├── crossbeam-deque v0.7.4
│   │   │   ├── crossbeam-epoch v0.8.2
│   │   │   │   ├── cfg-if v0.1.10
│   │   │   │   ├── crossbeam-utils v0.7.2 (*)
│   │   │   │   ├── lazy_static v1.5.0
│   │   │   │   ├── maybe-uninit v2.0.0
│   │   │   │   ├── memoffset v0.5.6
│   │   │   │   │   [build-dependencies]
│   │   │   │   │   └── autocfg v1.5.0
│   │   │   │   └── scopeguard v1.2.0
│   │   │   │   [build-dependencies]
│   │   │   │   └── autocfg v1.5.0
│   │   │   ├── crossbeam-utils v0.7.2 (*)
│   │   │   └── maybe-uninit v2.0.0
│   │   ├── crossbeam-epoch v0.8.2 (*)
│   │   ├── crossbeam-queue v0.2.3
│   │   │   ├── cfg-if v0.1.10
│   │   │   ├── crossbeam-utils v0.7.2 (*)
│   │   │   └── maybe-uninit v2.0.0
│   │   └── crossbeam-utils v0.7.2 (*)
│   ├── futures v0.3.31
│   │   ├── futures-channel v0.3.31
│   │   │   ├── futures-core v0.3.31
│   │   │   └── futures-sink v0.3.31
│   │   ├── futures-core v0.3.31
│   │   ├── futures-executor v0.3.31
│   │   │   ├── futures-core v0.3.31
│   │   │   ├── futures-task v0.3.31
│   │   │   ├── futures-util v0.3.31
│   │   │   │   ├── futures-channel v0.3.31 (*)
│   │   │   │   ├── futures-core v0.3.31
│   │   │   │   ├── futures-io v0.3.31
│   │   │   │   ├── futures-macro v0.3.31 (proc-macro)
│   │   │   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   │   │   ├── quote v1.0.40 (*)
│   │   │   │   │   └── syn v2.0.106 (*)
│   │   │   │   ├── futures-sink v0.3.31
│   │   │   │   ├── futures-task v0.3.31
│   │   │   │   ├── memchr v2.7.5
│   │   │   │   ├── pin-project-lite v0.2.16
│   │   │   │   ├── pin-utils v0.1.0
│   │   │   │   └── slab v0.4.11
│   │   │   └── num_cpus v1.17.0
│   │   │       └── libc v0.2.175
│   │   ├── futures-io v0.3.31
│   │   ├── futures-sink v0.3.31
│   │   ├── futures-task v0.3.31
│   │   └── futures-util v0.3.31 (*)
│   ├── hex v0.4.3
│   ├── lazy_static v1.5.0
│   ├── num_cpus v1.17.0 (*)
│   ├── pairing_ce v0.28.6
│   │   ├── byteorder v1.5.0
│   │   ├── cfg-if v1.0.3
│   │   ├── ff_ce v0.14.3
│   │   │   ├── byteorder v1.5.0
│   │   │   ├── ff_derive_ce v0.11.2 (proc-macro)
│   │   │   │   ├── num-bigint v0.4.6
│   │   │   │   │   ├── num-integer v0.1.46
│   │   │   │   │   │   └── num-traits v0.2.19
│   │   │   │   │   │       [build-dependencies]
│   │   │   │   │   │       └── autocfg v1.5.0
│   │   │   │   │   └── num-traits v0.2.19 (*)
│   │   │   │   ├── num-integer v0.1.46 (*)
│   │   │   │   ├── num-traits v0.2.19 (*)
│   │   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   │   ├── quote v1.0.40 (*)
│   │   │   │   ├── serde v1.0.219
│   │   │   │   │   └── serde_derive v1.0.219 (proc-macro) (*)
│   │   │   │   └── syn v1.0.109
│   │   │   │       ├── proc-macro2 v1.0.101 (*)
│   │   │   │       ├── quote v1.0.40 (*)
│   │   │   │       └── unicode-ident v1.0.19
│   │   │   ├── hex v0.4.3
│   │   │   ├── rand v0.4.6
│   │   │   │   └── libc v0.2.175
│   │   │   └── serde v1.0.219 (*)
│   │   ├── rand v0.4.6 (*)
│   │   └── serde v1.0.219 (*)
│   ├── rand v0.4.6 (*)
│   ├── serde v1.0.219 (*)
│   ├── smallvec v1.15.1
│   └── tiny-keccak v1.5.0
│       └── crunchy v0.2.4
├── bincode v1.3.3
│   └── serde v1.0.219 (*)
├── blake3 v1.8.2
│   ├── arrayref v0.3.9
│   ├── arrayvec v0.7.6
│   ├── cfg-if v1.0.3
│   └── constant_time_eq v0.3.1
│   [build-dependencies]
│   └── cc v1.2.36
│       ├── find-msvc-tools v0.1.1
│       ├── jobserver v0.1.34
│       │   └── libc v0.2.175
│       ├── libc v0.2.175
│       └── shlex v1.3.0
├── bridges v0.1.0 (/workspace/the-block/bridges)
│   ├── blake3 v1.8.2 (*)
│   ├── hex v0.4.3
│   ├── serde v1.0.219 (*)
│   └── serde_json v1.0.143
│       ├── itoa v1.0.15
│       ├── memchr v2.7.5
│       ├── ryu v1.0.20
│       └── serde v1.0.219 (*)
├── bytes v1.10.1
├── chacha20poly1305 v0.10.1
│   ├── aead v0.5.2
│   │   ├── crypto-common v0.1.6
│   │   │   ├── generic-array v0.14.7
│   │   │   │   └── typenum v1.18.0
│   │   │   │   [build-dependencies]
│   │   │   │   └── version_check v0.9.5
│   │   │   ├── rand_core v0.6.4
│   │   │   │   └── getrandom v0.2.16
│   │   │   │       ├── cfg-if v1.0.3
│   │   │   │       └── libc v0.2.175
│   │   │   └── typenum v1.18.0
│   │   └── generic-array v0.14.7 (*)
│   ├── chacha20 v0.9.1
│   │   ├── cfg-if v1.0.3
│   │   ├── cipher v0.4.4
│   │   │   ├── crypto-common v0.1.6 (*)
│   │   │   ├── inout v0.1.4
│   │   │   │   └── generic-array v0.14.7 (*)
│   │   │   └── zeroize v1.8.1
│   │   │       └── zeroize_derive v1.4.2 (proc-macro)
│   │   │           ├── proc-macro2 v1.0.101 (*)
│   │   │           ├── quote v1.0.40 (*)
│   │   │           └── syn v2.0.106 (*)
│   │   └── cpufeatures v0.2.17
│   ├── cipher v0.4.4 (*)
│   ├── poly1305 v0.8.0
│   │   ├── cpufeatures v0.2.17
│   │   ├── opaque-debug v0.3.1
│   │   └── universal-hash v0.5.1
│   │       ├── crypto-common v0.1.6 (*)
│   │       └── subtle v2.6.1
│   └── zeroize v1.8.1 (*)
├── clap v4.5.47
│   ├── clap_builder v4.5.47
│   │   ├── anstream v0.6.20
│   │   │   ├── anstyle v1.0.11
│   │   │   ├── anstyle-parse v0.2.7
│   │   │   │   └── utf8parse v0.2.2
│   │   │   ├── anstyle-query v1.1.4
│   │   │   ├── colorchoice v1.0.4
│   │   │   ├── is_terminal_polyfill v1.70.1
│   │   │   └── utf8parse v0.2.2
│   │   ├── anstyle v1.0.11
│   │   ├── clap_lex v0.7.5
│   │   └── strsim v0.11.1
│   └── clap_derive v4.5.47 (proc-macro)
│       ├── heck v0.5.0
│       ├── proc-macro2 v1.0.101 (*)
│       ├── quote v1.0.40 (*)
│       └── syn v2.0.106 (*)
├── clap_complete v4.5.57
│   └── clap v4.5.47 (*)
├── colored v2.2.0
│   └── lazy_static v1.5.0
├── crc32fast v1.5.0
│   └── cfg-if v1.0.3
├── dashmap v5.5.3
│   ├── cfg-if v1.0.3
│   ├── hashbrown v0.14.5
│   │   ├── ahash v0.8.12
│   │   │   ├── cfg-if v1.0.3
│   │   │   ├── getrandom v0.3.3
│   │   │   │   ├── cfg-if v1.0.3
│   │   │   │   └── libc v0.2.175
│   │   │   ├── once_cell v1.21.3
│   │   │   └── zerocopy v0.8.27
│   │   │   [build-dependencies]
│   │   │   └── version_check v0.9.5
│   │   └── allocator-api2 v0.2.21
│   ├── lock_api v0.4.13
│   │   └── scopeguard v1.2.0
│   │   [build-dependencies]
│   │   └── autocfg v1.5.0
│   ├── once_cell v1.21.3
│   └── parking_lot_core v0.9.11
│       ├── cfg-if v1.0.3
│       ├── libc v0.2.175
│       └── smallvec v1.15.1
├── dex v0.1.0 (/workspace/the-block/dex)
│   ├── blake3 v1.8.2 (*)
│   ├── hex v0.4.3
│   ├── serde v1.0.219 (*)
│   ├── sha3 v0.10.8
│   │   ├── digest v0.10.7
│   │   │   ├── block-buffer v0.10.4
│   │   │   │   └── generic-array v0.14.7 (*)
│   │   │   ├── crypto-common v0.1.6 (*)
│   │   │   └── subtle v2.6.1
│   │   └── keccak v0.1.5
│   └── subtle v2.6.1
├── dirs v5.0.1
│   └── dirs-sys v0.4.1
│       ├── libc v0.2.175
│       └── option-ext v0.2.0
├── ed25519-dalek v2.2.0
│   ├── curve25519-dalek v4.1.3
│   │   ├── cfg-if v1.0.3
│   │   ├── cpufeatures v0.2.17
│   │   ├── curve25519-dalek-derive v0.1.1 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   ├── quote v1.0.40 (*)
│   │   │   └── syn v2.0.106 (*)
│   │   ├── digest v0.10.7 (*)
│   │   ├── subtle v2.6.1
│   │   └── zeroize v1.8.1 (*)
│   │   [build-dependencies]
│   │   └── rustc_version v0.4.1
│   │       └── semver v1.0.26
│   ├── ed25519 v2.2.3
│   │   └── signature v2.2.0
│   ├── sha2 v0.10.9
│   │   ├── cfg-if v1.0.3
│   │   ├── cpufeatures v0.2.17
│   │   └── digest v0.10.7 (*)
│   ├── subtle v2.6.1
│   └── zeroize v1.8.1 (*)
├── flate2 v1.1.2
│   ├── crc32fast v1.5.0 (*)
│   └── miniz_oxide v0.8.9
│       └── adler2 v2.0.1
├── fs2 v0.4.3
│   └── libc v0.2.175
├── futures v0.3.31 (*)
├── hdrhistogram v7.5.4
│   ├── base64 v0.21.7
│   ├── byteorder v1.5.0
│   ├── crossbeam-channel v0.5.15
│   │   └── crossbeam-utils v0.8.21
│   ├── flate2 v1.1.2 (*)
│   ├── nom v7.1.3
│   │   ├── memchr v2.7.5
│   │   └── minimal-lexical v0.2.1
│   └── num-traits v0.2.19
│       └── libm v0.2.15
│       [build-dependencies]
│       └── autocfg v1.5.0
├── hex v0.4.3
├── httparse v1.10.1
├── runtime::ws (workspace WebSocket stack)
│   ├── base64 v0.21.7
│   ├── rand v0.8.5
│   └── sha1 v0.10.6

├── indexmap v2.11.1 (*)
├── inflation v0.1.0 (/workspace/the-block/inflation)
│   ├── bellman_ce v0.8.0 (*)
│   ├── rand v0.4.6 (*)
│   ├── serde v1.0.219 (*)
│   └── serde_json v1.0.143 (*)
├── jurisdiction v0.1.0 (/workspace/the-block/crates/jurisdiction)
│   ├── base64 v0.22.1
│   ├── log v0.4.28 (*)
│   ├── serde v1.0.219 (*)
│   └── serde_json v1.0.143 (*)
├── ledger v0.1.0 (/workspace/the-block/ledger)
│   ├── blake3 v1.8.2 (*)
│   ├── clap v4.5.47 (*)
│   ├── serde v1.0.219 (*)
│   └── serde_json v1.0.143 (*)
├── libp2p v0.52.4
│   ├── bytes v1.10.1
│   ├── either v1.15.0
│   ├── futures v0.3.31 (*)
│   ├── futures-timer v3.0.3
│   ├── getrandom v0.2.16 (*)
│   ├── instant v0.1.13
│   │   └── cfg-if v1.0.3
│   ├── libp2p-allow-block-list v0.2.0
│   │   ├── libp2p-core v0.40.1
│   │   │   ├── either v1.15.0
│   │   │   ├── fnv v1.0.7
│   │   │   ├── futures v0.3.31 (*)
│   │   │   ├── futures-timer v3.0.3
│   │   │   ├── instant v0.1.13 (*)
│   │   │   ├── libp2p-identity v0.2.12
│   │   │   │   ├── bs58 v0.5.1
│   │   │   │   ├── ed25519-dalek v2.2.0 (*)
│   │   │   │   ├── hkdf v0.12.4
│   │   │   │   │   └── hmac v0.12.1
│   │   │   │   │       └── digest v0.10.7 (*)
│   │   │   │   ├── multihash v0.19.3
│   │   │   │   │   ├── core2 v0.4.0
│   │   │   │   │   │   └── memchr v2.7.5
│   │   │   │   │   └── unsigned-varint v0.8.0
│   │   │   │   ├── quick-protobuf v0.8.1
│   │   │   │   │   └── byteorder v1.5.0
│   │   │   │   ├── rand v0.8.5 (*)
│   │   │   │   ├── sha2 v0.10.9 (*)
│   │   │   │   ├── thiserror v2.0.16
│   │   │   │   │   └── thiserror-impl v2.0.16 (proc-macro)
│   │   │   │   │       ├── proc-macro2 v1.0.101 (*)
│   │   │   │   │       ├── quote v1.0.40 (*)
│   │   │   │   │       └── syn v2.0.106 (*)
│   │   │   │   ├── tracing v0.1.41 (*)
│   │   │   │   └── zeroize v1.8.1 (*)
│   │   │   ├── log v0.4.28 (*)
│   │   │   ├── multiaddr v0.18.2
│   │   │   │   ├── arrayref v0.3.9
│   │   │   │   ├── byteorder v1.5.0
│   │   │   │   ├── data-encoding v2.9.0
│   │   │   │   ├── libp2p-identity v0.2.12 (*)
│   │   │   │   ├── multibase v0.9.1
│   │   │   │   │   ├── base-x v0.2.11
│   │   │   │   │   ├── data-encoding v2.9.0
│   │   │   │   │   └── data-encoding-macro v0.1.18
│   │   │   │   │       ├── data-encoding v2.9.0
│   │   │   │   │       └── data-encoding-macro-internal v0.1.16 (proc-macro)
│   │   │   │   │           ├── data-encoding v2.9.0
│   │   │   │   │           └── syn v2.0.106 (*)
│   │   │   │   ├── multihash v0.19.3 (*)
│   │   │   │   ├── percent-encoding v2.3.2
│   │   │   │   ├── serde v1.0.219 (*)
│   │   │   │   ├── static_assertions v1.1.0
│   │   │   │   ├── unsigned-varint v0.8.0
│   │   │   │   └── url v2.5.7 (*)
│   │   │   ├── multihash v0.19.3 (*)
│   │   │   ├── multistream-select v0.13.0
│   │   │   │   ├── bytes v1.10.1
│   │   │   │   ├── futures v0.3.31 (*)
│   │   │   │   ├── log v0.4.28 (*)
│   │   │   │   ├── pin-project v1.1.10
│   │   │   │   │   └── pin-project-internal v1.1.10 (proc-macro)
│   │   │   │   │       ├── proc-macro2 v1.0.101 (*)
│   │   │   │   │       ├── quote v1.0.40 (*)
│   │   │   │   │       └── syn v2.0.106 (*)
│   │   │   │   ├── smallvec v1.15.1
│   │   │   │   └── unsigned-varint v0.7.2
│   │   │   │       ├── asynchronous-codec v0.6.2
│   │   │   │       │   ├── bytes v1.10.1
│   │   │   │       │   ├── futures-sink v0.3.31
│   │   │   │       │   ├── futures-util v0.3.31 (*)
│   │   │   │       │   ├── memchr v2.7.5
│   │   │   │       │   └── pin-project-lite v0.2.16
│   │   │   │       └── bytes v1.10.1
│   │   │   ├── once_cell v1.21.3
│   │   │   ├── parking_lot v0.12.4
│   │   │   │   ├── lock_api v0.4.13 (*)
│   │   │   │   └── parking_lot_core v0.9.11 (*)
│   │   │   ├── pin-project v1.1.10 (*)
│   │   │   ├── quick-protobuf v0.8.1 (*)
│   │   │   ├── rand v0.8.5 (*)
│   │   │   ├── rw-stream-sink v0.4.0
│   │   │   │   ├── futures v0.3.31 (*)
│   │   │   │   ├── pin-project v1.1.10 (*)
│   │   │   │   └── static_assertions v1.1.0
│   │   │   ├── smallvec v1.15.1
│   │   │   ├── thiserror v1.0.69 (*)
│   │   │   ├── unsigned-varint v0.7.2 (*)
│   │   │   └── void v1.0.2
│   │   ├── libp2p-identity v0.2.12 (*)
│   │   ├── libp2p-swarm v0.43.7
│   │   │   ├── either v1.15.0
│   │   │   ├── fnv v1.0.7
│   │   │   ├── futures v0.3.31 (*)
│   │   │   ├── futures-timer v3.0.3
│   │   │   ├── instant v0.1.13 (*)
│   │   │   ├── libp2p-core v0.40.1 (*)
│   │   │   ├── libp2p-identity v0.2.12 (*)
│   │   │   ├── log v0.4.28 (*)
│   │   │   ├── multistream-select v0.13.0 (*)
│   │   │   ├── once_cell v1.21.3
│   │   │   ├── rand v0.8.5 (*)
│   │   │   ├── smallvec v1.15.1
│   │   │   ├── tokio v1.47.1 (*)
│   │   │   └── void v1.0.2
│   │   └── void v1.0.2
│   ├── libp2p-connection-limits v0.2.1
│   │   ├── libp2p-core v0.40.1 (*)
│   │   ├── libp2p-identity v0.2.12 (*)
│   │   ├── libp2p-swarm v0.43.7 (*)
│   │   └── void v1.0.2
│   ├── libp2p-core v0.40.1 (*)
│   ├── libp2p-identity v0.2.12 (*)
│   ├── libp2p-kad v0.44.6
│   │   ├── arrayvec v0.7.6
│   │   ├── asynchronous-codec v0.6.2 (*)
│   │   ├── bytes v1.10.1
│   │   ├── either v1.15.0
│   │   ├── fnv v1.0.7
│   │   ├── futures v0.3.31 (*)
│   │   ├── futures-timer v3.0.3
│   │   ├── instant v0.1.13 (*)
│   │   ├── libp2p-core v0.40.1 (*)
│   │   ├── libp2p-identity v0.2.12 (*)
│   │   ├── libp2p-swarm v0.43.7 (*)
│   │   ├── log v0.4.28 (*)
│   │   ├── quick-protobuf v0.8.1 (*)
│   │   ├── quick-protobuf-codec v0.2.0
│   │   │   ├── asynchronous-codec v0.6.2 (*)
│   │   │   ├── bytes v1.10.1
│   │   │   ├── quick-protobuf v0.8.1 (*)
│   │   │   ├── thiserror v1.0.69 (*)
│   │   │   └── unsigned-varint v0.7.2 (*)
│   │   ├── rand v0.8.5 (*)
│   │   ├── sha2 v0.10.9 (*)
│   │   ├── smallvec v1.15.1
│   │   ├── thiserror v1.0.69 (*)
│   │   ├── uint v0.9.5
│   │   │   ├── byteorder v1.5.0
│   │   │   ├── crunchy v0.2.4
│   │   │   ├── hex v0.4.3
│   │   │   └── static_assertions v1.1.0
│   │   ├── unsigned-varint v0.7.2 (*)
│   │   └── void v1.0.2
│   ├── libp2p-noise v0.43.2
│   │   ├── bytes v1.10.1
│   │   ├── curve25519-dalek v4.1.3 (*)
│   │   ├── futures v0.3.31 (*)
│   │   ├── libp2p-core v0.40.1 (*)
│   │   ├── libp2p-identity v0.2.12 (*)
│   │   ├── log v0.4.28 (*)
│   │   ├── multiaddr v0.18.2 (*)
│   │   ├── multihash v0.19.3 (*)
│   │   ├── once_cell v1.21.3
│   │   ├── quick-protobuf v0.8.1 (*)
│   │   ├── rand v0.8.5 (*)
│   │   ├── sha2 v0.10.9 (*)
│   │   ├── snow v0.9.6
│   │   │   ├── rand_core v0.6.4 (*)
│   │   │   ├── ring v0.17.14
│   │   │   │   ├── cfg-if v1.0.3
│   │   │   │   ├── getrandom v0.2.16 (*)
│   │   │   │   └── untrusted v0.9.0
│   │   │   │   [build-dependencies]
│   │   │   │   └── cc v1.2.36 (*)
│   │   │   └── subtle v2.6.1
│   │   │   [build-dependencies]
│   │   │   └── rustc_version v0.4.1 (*)
│   │   ├── static_assertions v1.1.0
│   │   ├── thiserror v1.0.69 (*)
│   │   ├── x25519-dalek v2.0.1
│   │   │   ├── curve25519-dalek v4.1.3 (*)
│   │   │   ├── rand_core v0.6.4 (*)
│   │   │   └── zeroize v1.8.1 (*)
│   │   └── zeroize v1.8.1 (*)
│   ├── libp2p-swarm v0.43.7 (*)
│   ├── libp2p-tcp v0.40.1
│   │   ├── futures v0.3.31 (*)
│   │   ├── futures-timer v3.0.3
│   │   ├── if-watch v3.2.1
│   │   │   ├── fnv v1.0.7
│   │   │   ├── futures v0.3.31 (*)
│   │   │   ├── ipnet v2.11.0
│   │   │   ├── log v0.4.28 (*)
│   │   │   ├── netlink-packet-core v0.7.0
│   │   │   │   ├── anyhow v1.0.99
│   │   │   │   ├── byteorder v1.5.0
│   │   │   │   └── netlink-packet-utils v0.5.2
│   │   │   │       ├── anyhow v1.0.99
│   │   │   │       ├── byteorder v1.5.0
│   │   │   │       ├── paste v1.0.15 (proc-macro)
│   │   │   │       └── thiserror v1.0.69 (*)
│   │   │   ├── netlink-packet-route v0.17.1
│   │   │   │   ├── anyhow v1.0.99
│   │   │   │   ├── bitflags v1.3.2
│   │   │   │   ├── byteorder v1.5.0
│   │   │   │   ├── libc v0.2.175
│   │   │   │   ├── netlink-packet-core v0.7.0 (*)
│   │   │   │   └── netlink-packet-utils v0.5.2 (*)
│   │   │   ├── netlink-proto v0.11.5
│   │   │   │   ├── bytes v1.10.1
│   │   │   │   ├── futures v0.3.31 (*)
│   │   │   │   ├── log v0.4.28 (*)
│   │   │   │   ├── netlink-packet-core v0.7.0 (*)
│   │   │   │   ├── netlink-sys v0.8.7
│   │   │   │   │   ├── bytes v1.10.1
│   │   │   │   │   ├── futures v0.3.31 (*)
│   │   │   │   │   ├── libc v0.2.175
│   │   │   │   │   ├── log v0.4.28 (*)
│   │   │   │   │   └── tokio v1.47.1 (*)
│   │   │   │   └── thiserror v2.0.16 (*)
│   │   │   ├── netlink-sys v0.8.7 (*)
│   │   │   └── rtnetlink v0.13.1
│   │   │       ├── futures v0.3.31 (*)
│   │   │       ├── log v0.4.28 (*)
│   │   │       ├── netlink-packet-core v0.7.0 (*)
│   │   │       ├── netlink-packet-route v0.17.1 (*)
│   │   │       ├── netlink-packet-utils v0.5.2 (*)
│   │   │       ├── netlink-proto v0.11.5 (*)
│   │   │       ├── netlink-sys v0.8.7 (*)
│   │   │       ├── nix v0.26.4
│   │   │       │   ├── bitflags v1.3.2
│   │   │       │   ├── cfg-if v1.0.3
│   │   │       │   └── libc v0.2.175
│   │   │       ├── thiserror v1.0.69 (*)
│   │   │       └── tokio v1.47.1 (*)
│   │   ├── libc v0.2.175
│   │   ├── libp2p-core v0.40.1 (*)
│   │   ├── libp2p-identity v0.2.12 (*)
│   │   ├── log v0.4.28 (*)
│   │   ├── socket2 v0.5.10 (*)
│   │   └── tokio v1.47.1 (*)
│   ├── libp2p-yamux v0.44.1
│   │   ├── futures v0.3.31 (*)
│   │   ├── libp2p-core v0.40.1 (*)
│   │   ├── log v0.4.28 (*)
│   │   ├── thiserror v1.0.69 (*)
│   │   └── yamux v0.12.1
│   │       ├── futures v0.3.31 (*)
│   │       ├── log v0.4.28 (*)
│   │       ├── nohash-hasher v0.2.0
│   │       ├── parking_lot v0.12.4 (*)
│   │       ├── pin-project v1.1.10 (*)
│   │       ├── rand v0.8.5 (*)
│   │       └── static_assertions v1.1.0
│   ├── multiaddr v0.18.2 (*)
│   ├── pin-project v1.1.10 (*)
│   ├── rw-stream-sink v0.4.0 (*)
│   └── thiserror v1.0.69 (*)
├── lru v0.11.1
│   └── hashbrown v0.14.5 (*)
├── nalgebra v0.32.6
│   ├── approx v0.5.1
│   │   └── num-traits v0.2.19 (*)
│   ├── matrixmultiply v0.3.10
│   │   └── rawpointer v0.2.1
│   │   [build-dependencies]
│   │   └── autocfg v1.5.0
│   ├── nalgebra-macros v0.2.2 (proc-macro)
│   │   ├── proc-macro2 v1.0.101 (*)
│   │   ├── quote v1.0.40 (*)
│   │   └── syn v2.0.106 (*)
│   ├── num-complex v0.4.6
│   │   └── num-traits v0.2.19 (*)
│   ├── num-rational v0.4.2
│   │   ├── num-integer v0.1.46 (*)
│   │   └── num-traits v0.2.19 (*)
│   ├── num-traits v0.2.19 (*)
│   ├── simba v0.8.1
│   │   ├── approx v0.5.1 (*)
│   │   ├── num-complex v0.4.6 (*)
│   │   ├── num-traits v0.2.19 (*)
│   │   ├── paste v1.0.15 (proc-macro)
│   │   └── wide v0.7.33
│   │       ├── bytemuck v1.23.2
│   │       └── safe_arch v0.7.4
│   │           └── bytemuck v1.23.2
│   └── typenum v1.18.0
├── nix v0.27.1
│   ├── bitflags v2.9.4
│   ├── cfg-if v1.0.3
│   └── libc v0.2.175
├── notify v6.1.1
│   ├── crossbeam-channel v0.5.15 (*)
│   ├── filetime v0.2.26
│   │   ├── cfg-if v1.0.3
│   │   └── libc v0.2.175
│   ├── inotify v0.9.6
│   │   ├── bitflags v1.3.2
│   │   ├── inotify-sys v0.1.5
│   │   │   └── libc v0.2.175
│   │   └── libc v0.2.175
│   ├── libc v0.2.175
│   ├── log v0.4.28 (*)
│   ├── mio v0.8.11
│   │   ├── libc v0.2.175
│   │   └── log v0.4.28 (*)
│   └── walkdir v2.5.0
│       └── same-file v1.0.6
├── num_cpus v1.17.0 (*)
├── once_cell v1.21.3
├── parking_lot v0.12.4 (*)
├── pprof v0.13.0
│   ├── backtrace v0.3.75
│   │   ├── addr2line v0.24.2
│   │   │   └── gimli v0.31.1
│   │   ├── cfg-if v1.0.3
│   │   ├── libc v0.2.175
│   │   ├── miniz_oxide v0.8.9 (*)
│   │   ├── object v0.36.7
│   │   │   └── memchr v2.7.5
│   │   └── rustc-demangle v0.1.26
│   ├── cfg-if v1.0.3
│   ├── findshlibs v0.10.2
│   │   └── libc v0.2.175
│   │   [build-dependencies]
│   │   └── cc v1.2.36 (*)
│   ├── inferno v0.11.21
│   │   ├── ahash v0.8.12 (*)
│   │   ├── indexmap v2.11.1 (*)
│   │   ├── is-terminal v0.4.16
│   │   │   └── libc v0.2.175
│   │   ├── itoa v1.0.15
│   │   ├── log v0.4.28 (*)
│   │   ├── num-format v0.4.4
│   │   │   ├── arrayvec v0.7.6
│   │   │   └── itoa v1.0.15
│   │   ├── once_cell v1.21.3
│   │   ├── quick-xml v0.26.0
│   │   │   └── memchr v2.7.5
│   │   ├── rgb v0.8.52
│   │   │   └── bytemuck v1.23.2
│   │   └── str_stack v0.1.0
│   ├── libc v0.2.175
│   ├── log v0.4.28 (*)
│   ├── nix v0.26.4 (*)
│   ├── once_cell v1.21.3
│   ├── parking_lot v0.12.4 (*)
│   ├── smallvec v1.15.1
│   ├── symbolic-demangle v12.16.2
│   │   ├── cpp_demangle v0.4.4
│   │   │   └── cfg-if v1.0.3
│   │   ├── rustc-demangle v0.1.26
│   │   └── symbolic-common v12.16.2
│   │       ├── debugid v0.8.0
│   │       │   └── uuid v1.18.1
│   │       │       └── getrandom v0.3.3 (*)
│   │       ├── memmap2 v0.9.8
│   │       │   └── libc v0.2.175
│   │       ├── stable_deref_trait v1.2.0
│   │       └── uuid v1.18.1 (*)
│   ├── tempfile v3.22.0
│   │   ├── fastrand v2.3.0
│   │   ├── getrandom v0.3.3 (*)
│   │   ├── once_cell v1.21.3
│   │   └── rustix v1.1.2
│   │       ├── bitflags v2.9.4
│   │       └── linux-raw-sys v0.11.0
│   └── thiserror v1.0.69 (*)
├── procfs v0.15.1
│   ├── bitflags v1.3.2
│   ├── byteorder v1.5.0
│   ├── chrono v0.4.42
│   │   ├── iana-time-zone v0.1.63
│   │   └── num-traits v0.2.19 (*)
│   ├── flate2 v1.1.2 (*)
│   ├── hex v0.4.3
│   ├── lazy_static v1.5.0
│   └── rustix v0.36.17
│       ├── bitflags v1.3.2
│       ├── io-lifetimes v1.0.11
│       │   └── libc v0.2.175
│       ├── libc v0.2.175
│       └── linux-raw-sys v0.1.4
├── pyo3 v0.24.2
│   ├── cfg-if v1.0.3
│   ├── indoc v2.0.6 (proc-macro)
│   ├── libc v0.2.175
│   ├── memoffset v0.9.1
│   │   [build-dependencies]
│   │   └── autocfg v1.5.0
│   ├── once_cell v1.21.3
│   ├── pyo3-ffi v0.24.2
│   │   └── libc v0.2.175
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
│   │   │   ├── quote v1.0.40 (*)
│   │   │   └── syn v2.0.106 (*)
│   │   │   [build-dependencies]
│   │   │   └── pyo3-build-config v0.24.2 (*)
│   │   ├── quote v1.0.40 (*)
│   │   └── syn v2.0.106 (*)
│   └── unindent v0.2.4
│   [build-dependencies]
│   └── pyo3-build-config v0.24.2 (*)
├── rand v0.8.5 (*)
├── rand_core v0.6.4 (*)
├── rayon v1.11.0
│   ├── either v1.15.0
│   └── rayon-core v1.13.0
│       ├── crossbeam-deque v0.8.6
│       │   ├── crossbeam-epoch v0.9.18
│       │   │   └── crossbeam-utils v0.8.21
│       │   └── crossbeam-utils v0.8.21
│       └── crossbeam-utils v0.8.21
├── regex v1.11.2
│   ├── aho-corasick v1.1.3
│   │   └── memchr v2.7.5
│   ├── memchr v2.7.5
│   ├── regex-automata v0.4.10
│   │   ├── aho-corasick v1.1.3 (*)
│   │   ├── memchr v2.7.5
│   │   └── regex-syntax v0.8.6
│   └── regex-syntax v0.8.6
├── reqwest v0.11.27
│   ├── base64 v0.21.7
│   ├── bytes v1.10.1
│   ├── encoding_rs v0.8.35
│   │   └── cfg-if v1.0.3
│   ├── futures-core v0.3.31
│   ├── futures-util v0.3.31 (*)
│   ├── h2 v0.3.27 (*)
│   ├── http v0.2.12 (*)
│   ├── http-body v0.4.6 (*)
│   ├── hyper v0.14.32 (*)
│   ├── hyper-tls v0.5.0
│   │   ├── bytes v1.10.1
│   │   ├── hyper v0.14.32 (*)
│   │   ├── native-tls v0.2.14 (*)
│   │   ├── tokio v1.47.1 (*)
│   │   └── tokio-native-tls v0.3.1
│   │       ├── native-tls v0.2.14 (*)
│   │       └── tokio v1.47.1 (*)
│   ├── ipnet v2.11.0
│   ├── log v0.4.28 (*)
│   ├── mime v0.3.17
│   ├── native-tls v0.2.14 (*)
│   ├── once_cell v1.21.3
│   ├── percent-encoding v2.3.2
│   ├── pin-project-lite v0.2.16
│   ├── rustls-pemfile v1.0.4
│   │   └── base64 v0.21.7
│   ├── serde v1.0.219 (*)
│   ├── serde_json v1.0.143 (*)
│   ├── serde_urlencoded v0.7.1
│   │   ├── form_urlencoded v1.2.2 (*)
│   │   ├── itoa v1.0.15
│   │   ├── ryu v1.0.20
│   │   └── serde v1.0.219 (*)
│   ├── sync_wrapper v0.1.2
│   ├── tokio v1.47.1 (*)
│   ├── tokio-native-tls v0.3.1 (*)
│   ├── tower-service v0.3.3
│   └── url v2.5.7 (*)
├── ripemd v0.1.3
│   └── digest v0.10.7 (*)
├── rocksdb v0.21.0
│   ├── libc v0.2.175
│   └── librocksdb-sys v0.11.0+8.1.1
│       ├── bzip2-sys v0.1.13+1.0.8
│       │   [build-dependencies]
│       │   ├── cc v1.2.36 (*)
│       │   └── pkg-config v0.3.32
│       ├── libc v0.2.175
│       ├── libz-sys v1.1.22
│       │   [build-dependencies]
│       │   ├── cc v1.2.36 (*)
│       │   ├── pkg-config v0.3.32
│       │   └── vcpkg v0.2.15
│       ├── lz4-sys v1.11.1+lz4-1.10.0
│       │   └── libc v0.2.175
│       │   [build-dependencies]
│       │   └── cc v1.2.36 (*)
│       └── zstd-sys v2.0.16+zstd.1.5.7
│           [build-dependencies]
│           ├── bindgen v0.72.1
│           │   ├── bitflags v2.9.4
│           │   ├── cexpr v0.6.0
│           │   │   └── nom v7.1.3
│           │   │       ├── memchr v2.7.5
│           │   │       └── minimal-lexical v0.2.1
│           │   ├── clang-sys v1.8.1
│           │   │   ├── glob v0.3.3
│           │   │   ├── libc v0.2.175
│           │   │   └── libloading v0.8.8
│           │   │       └── cfg-if v1.0.3
│           │   │   [build-dependencies]
│           │   │   └── glob v0.3.3
│           │   ├── itertools v0.13.0
│           │   │   └── either v1.15.0
│           │   ├── proc-macro2 v1.0.101 (*)
│           │   ├── quote v1.0.40 (*)
│           │   ├── regex v1.11.2
│           │   │   ├── regex-automata v0.4.10
│           │   │   │   └── regex-syntax v0.8.6
│           │   │   └── regex-syntax v0.8.6
│           │   ├── rustc-hash v2.1.1
│           │   ├── shlex v1.3.0
│           │   └── syn v2.0.106 (*)
│           ├── cc v1.2.36 (*)
│           └── pkg-config v0.3.32
│       [build-dependencies]
│       ├── bindgen v0.65.1
│       │   ├── bitflags v1.3.2
│       │   ├── cexpr v0.6.0 (*)
│       │   ├── clang-sys v1.8.1 (*)
│       │   ├── lazy_static v1.5.0
│       │   ├── lazycell v1.3.0
│       │   ├── peeking_take_while v0.1.2
│       │   ├── prettyplease v0.2.37
│       │   │   ├── proc-macro2 v1.0.101 (*)
│       │   │   └── syn v2.0.106 (*)
│       │   ├── proc-macro2 v1.0.101 (*)
│       │   ├── quote v1.0.40 (*)
│       │   ├── regex v1.11.2 (*)
│       │   ├── rustc-hash v1.1.0
│       │   ├── shlex v1.3.0
│       │   └── syn v2.0.106 (*)
│       ├── cc v1.2.36 (*)
│       └── glob v0.3.3
├── rustdct v0.7.1
│   └── rustfft v6.4.0
│       ├── num-complex v0.4.6 (*)
│       ├── num-integer v0.1.46 (*)
│       ├── num-traits v0.2.19 (*)
│       ├── primal-check v0.3.4
│       │   └── num-integer v0.1.46 (*)
│       ├── strength_reduce v0.2.4
│       └── transpose v0.2.3
│           ├── num-integer v0.1.46 (*)
│           └── strength_reduce v0.2.4
├── rustls v0.21.12
│   ├── log v0.4.28 (*)
│   ├── ring v0.17.14 (*)
│   ├── rustls-webpki v0.101.7
│   │   ├── ring v0.17.14 (*)
│   │   └── untrusted v0.9.0
│   └── sct v0.7.1
│       ├── ring v0.17.14 (*)
│       └── untrusted v0.9.0
├── serde v1.0.219 (*)
├── serde_bytes v0.11.17
│   └── serde v1.0.219 (*)
├── serde_cbor v0.11.2
│   ├── half v1.8.3
│   └── serde v1.0.219 (*)
├── serde_json v1.0.143 (*)
├── sha1 v0.10.6 (*)
├── sha3 v0.10.8 (*)
├── signal-hook v0.3.18
│   ├── libc v0.2.175
│   └── signal-hook-registry v1.4.6
│       └── libc v0.2.175
├── sled v0.34.7
│   ├── crc32fast v1.5.0 (*)
│   ├── crossbeam-epoch v0.9.18 (*)
│   ├── crossbeam-utils v0.8.21
│   ├── fs2 v0.4.3 (*)
│   ├── fxhash v0.2.1
│   │   └── byteorder v1.5.0
│   ├── libc v0.2.175
│   ├── log v0.4.28 (*)
│   └── parking_lot v0.11.2 (*)
├── state v0.1.0 (/workspace/the-block/state)
│   ├── bincode v1.3.3 (*)
│   ├── blake3 v1.8.2 (*)
│   ├── hex v0.4.3
│   ├── serde v1.0.219 (*)
│   └── thiserror v1.0.69 (*)
├── statrs v0.16.1
│   ├── approx v0.5.1 (*)
│   ├── lazy_static v1.5.0
│   ├── nalgebra v0.29.0
│   │   ├── approx v0.5.1 (*)
│   │   ├── matrixmultiply v0.3.10 (*)
│   │   ├── nalgebra-macros v0.1.0 (proc-macro)
│   │   │   ├── proc-macro2 v1.0.101 (*)
│   │   │   ├── quote v1.0.40 (*)
│   │   │   └── syn v1.0.109 (*)
│   │   ├── num-complex v0.4.6 (*)
│   │   ├── num-rational v0.4.2 (*)
│   │   ├── num-traits v0.2.19 (*)
│   │   ├── rand v0.8.5 (*)
│   │   ├── rand_distr v0.4.3
│   │   │   ├── num-traits v0.2.19 (*)
│   │   │   └── rand v0.8.5 (*)
│   │   ├── simba v0.6.0
│   │   │   ├── approx v0.5.1 (*)
│   │   │   ├── num-complex v0.4.6 (*)
│   │   │   ├── num-traits v0.2.19 (*)
│   │   │   ├── paste v1.0.15 (proc-macro)
│   │   │   └── wide v0.7.33 (*)
│   │   └── typenum v1.18.0
│   ├── num-traits v0.2.19 (*)
│   └── rand v0.8.5 (*)
├── storage v0.1.0 (/workspace/the-block/storage)
│   ├── serde v1.0.219 (*)
│   └── thiserror v1.0.69 (*)
├── subtle v2.6.1
├── tar v0.4.44
│   ├── filetime v0.2.26 (*)
│   ├── libc v0.2.175
│   └── xattr v1.5.1
│       └── rustix v1.1.2 (*)
├── tempfile v3.22.0 (*)
├── terminal_size v0.2.6
│   └── rustix v0.37.28
│       ├── bitflags v1.3.2
│       ├── io-lifetimes v1.0.11 (*)
│       ├── libc v0.2.175
│       └── linux-raw-sys v0.3.8
├── thiserror v1.0.69 (*)
├── tokio v1.47.1 (*)
├── runtime::ws (workspace WebSocket stack)
│   ├── base64 v0.21.7
│   ├── rand v0.8.5
│   └── sha1 v0.10.6

├── tokio-util v0.7.16 (*)
├── toml v0.8.23
│   ├── serde v1.0.219 (*)
│   ├── serde_spanned v0.6.9
│   │   └── serde v1.0.219 (*)
│   ├── toml_datetime v0.6.11
│   │   └── serde v1.0.219 (*)
│   └── toml_edit v0.22.27
│       ├── indexmap v2.11.1 (*)
│       ├── serde v1.0.219 (*)
│       ├── serde_spanned v0.6.9 (*)
│       ├── toml_datetime v0.6.11 (*)
│       ├── toml_write v0.1.2
│       └── winnow v0.7.13
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
│       │   └── regex-automata v0.4.10 (*)
│       ├── nu-ansi-term v0.50.1
│       ├── once_cell v1.21.3
│       ├── regex-automata v0.4.10 (*)
│       ├── serde v1.0.219 (*)
│       ├── serde_json v1.0.143 (*)
│       ├── sharded-slab v0.1.7
│       │   └── lazy_static v1.5.0
│       ├── smallvec v1.15.1
│       ├── thread_local v1.1.9
│       │   └── cfg-if v1.0.3
│       ├── tracing v0.1.41 (*)
│       ├── tracing-core v0.1.34 (*)
│       ├── tracing-log v0.2.0
│       │   ├── log v0.4.28 (*)
│       │   ├── once_cell v1.21.3
│       │   └── tracing-core v0.1.34 (*)
│       └── tracing-serde v0.2.0
│           ├── serde v1.0.219 (*)
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
│   ├── resolv-conf v0.7.4
│   ├── smallvec v1.15.1
│   ├── thiserror v1.0.69 (*)
│   ├── tokio v1.47.1 (*)
│   ├── tracing v0.1.41 (*)
│   └── trust-dns-proto v0.23.2
│       ├── async-trait v0.1.89 (proc-macro)
│       │   ├── proc-macro2 v1.0.101 (*)
│       │   ├── quote v1.0.40 (*)
│       │   └── syn v2.0.106 (*)
│       ├── cfg-if v1.0.3
│       ├── data-encoding v2.9.0
│       ├── enum-as-inner v0.6.1 (proc-macro)
│       │   ├── heck v0.5.0
│       │   ├── proc-macro2 v1.0.101 (*)
│       │   ├── quote v1.0.40 (*)
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
│       ├── smallvec v1.15.1
│       ├── thiserror v1.0.69 (*)
│       ├── tinyvec v1.10.0 (*)
│       ├── tokio v1.47.1 (*)
│       ├── tracing v0.1.41 (*)
│       └── url v2.5.7 (*)
├── runtime::ws (workspace WebSocket stack)
│   ├── base64 v0.21.7
│   ├── rand v0.8.5
│   └── sha1 v0.10.6

├── unicode-normalization v0.1.24 (*)
├── wallet v0.1.0 (/workspace/the-block/crates/wallet)
│   ├── ed25519-dalek v1.0.1
│   │   ├── curve25519-dalek v3.2.0
│   │   │   ├── byteorder v1.5.0
│   │   │   ├── digest v0.9.0
│   │   │   │   └── generic-array v0.14.7 (*)
│   │   │   ├── rand_core v0.5.1
│   │   │   │   └── getrandom v0.1.16
│   │   │   │       ├── cfg-if v1.0.3
│   │   │   │       └── libc v0.2.175
│   │   │   ├── subtle v2.6.1
│   │   │   └── zeroize v1.8.1 (*)
│   │   ├── ed25519 v1.5.3
│   │   │   └── signature v1.6.4
│   │   ├── rand v0.7.3
│   │   │   ├── getrandom v0.1.16 (*)
│   │   │   ├── libc v0.2.175
│   │   │   ├── rand_chacha v0.2.2
│   │   │   │   ├── ppv-lite86 v0.2.21 (*)
│   │   │   │   └── rand_core v0.5.1 (*)
│   │   │   └── rand_core v0.5.1 (*)
│   │   ├── serde v1.0.219 (*)
│   │   ├── sha2 v0.9.9
│   │   │   ├── block-buffer v0.9.0
│   │   │   │   └── generic-array v0.14.7 (*)
│   │   │   ├── cfg-if v1.0.3
│   │   │   ├── cpufeatures v0.2.17
│   │   │   ├── digest v0.9.0 (*)
│   │   │   └── opaque-debug v0.3.1
│   │   └── zeroize v1.8.1 (*)
│   ├── hex v0.4.3
│   ├── hkdf v0.12.4 (*)
│   ├── ledger v0.1.0 (/workspace/the-block/ledger) (*)
│   ├── native-tls v0.2.14 (*)
│   ├── once_cell v1.21.3
│   ├── rand v0.7.3 (*)
│   ├── reqwest v0.11.27 (*)
│   ├── serde v1.0.219 (*)
│   ├── serde_json v1.0.143 (*)
│   ├── sha2 v0.10.9 (*)
│   ├── subtle v2.6.1
│   ├── thiserror v1.0.69 (*)
│   ├── tracing v0.1.41 (*)
│   ├── runtime::ws (workspace WebSocket stack)
│   ├── url v2.5.7 (*)
│   └── uuid v1.18.1 (*)
├── xorfilter-rs v0.5.1
└── zstd v0.12.4
    └── zstd-safe v6.0.6
        ├── libc v0.2.175
        └── zstd-sys v2.0.16+zstd.1.5.7 (*)
[build-dependencies]
└── blake3 v1.8.2 (*)
[dev-dependencies]
├── arbitrary v1.4.2
│   └── derive_arbitrary v1.4.2 (proc-macro)
│       ├── proc-macro2 v1.0.101 (*)
│       ├── quote v1.0.40 (*)
│       └── syn v2.0.106 (*)
├── axum v0.7.9
│   ├── async-trait v0.1.89 (proc-macro) (*)
│   ├── axum-core v0.4.5
│   │   ├── async-trait v0.1.89 (proc-macro) (*)
│   │   ├── bytes v1.10.1
│   │   ├── futures-util v0.3.31 (*)
│   │   ├── http v1.3.1 (*)
│   │   ├── http-body v1.0.1
│   │   │   ├── bytes v1.10.1
│   │   │   └── http v1.3.1 (*)
│   │   ├── http-body-util v0.1.3
│   │   │   ├── bytes v1.10.1
│   │   │   ├── futures-core v0.3.31
│   │   │   ├── http v1.3.1 (*)
│   │   │   ├── http-body v1.0.1 (*)
│   │   │   └── pin-project-lite v0.2.16
│   │   ├── mime v0.3.17
│   │   ├── pin-project-lite v0.2.16
│   │   ├── rustversion v1.0.22 (proc-macro)
│   │   ├── sync_wrapper v1.0.2
│   │   ├── tower-layer v0.3.3
│   │   ├── tower-service v0.3.3
│   │   └── tracing v0.1.41 (*)
│   ├── bytes v1.10.1
│   ├── futures-util v0.3.31 (*)
│   ├── http v1.3.1 (*)
│   ├── http-body v1.0.1 (*)
│   ├── http-body-util v0.1.3 (*)
│   ├── hyper v1.7.0
│   │   ├── atomic-waker v1.1.2
│   │   ├── bytes v1.10.1
│   │   ├── futures-channel v0.3.31 (*)
│   │   ├── futures-core v0.3.31
│   │   ├── http v1.3.1 (*)
│   │   ├── http-body v1.0.1 (*)
│   │   ├── httparse v1.10.1
│   │   ├── httpdate v1.0.3
│   │   ├── itoa v1.0.15
│   │   ├── pin-project-lite v0.2.16
│   │   ├── pin-utils v0.1.0
│   │   ├── smallvec v1.15.1
│   │   └── tokio v1.47.1 (*)
│   ├── hyper-util v0.1.16
│   │   ├── bytes v1.10.1
│   │   ├── futures-core v0.3.31
│   │   ├── http v1.3.1 (*)
│   │   ├── http-body v1.0.1 (*)
│   │   ├── hyper v1.7.0 (*)
│   │   ├── pin-project-lite v0.2.16
│   │   ├── tokio v1.47.1 (*)
│   │   └── tower-service v0.3.3
│   ├── itoa v1.0.15
│   ├── matchit v0.7.3
│   ├── memchr v2.7.5
│   ├── mime v0.3.17
│   ├── percent-encoding v2.3.2
│   ├── pin-project-lite v0.2.16
│   ├── rustversion v1.0.22 (proc-macro)
│   ├── serde v1.0.219 (*)
│   ├── serde_json v1.0.143 (*)
│   ├── serde_path_to_error v0.1.17
│   │   ├── itoa v1.0.15
│   │   └── serde v1.0.219 (*)
│   ├── serde_urlencoded v0.7.1 (*)
│   ├── sync_wrapper v1.0.2
│   ├── tokio v1.47.1 (*)
│   ├── tower v0.5.2
│   │   ├── futures-core v0.3.31
│   │   ├── futures-util v0.3.31 (*)
│   │   ├── pin-project-lite v0.2.16
│   │   ├── sync_wrapper v1.0.2
│   │   ├── tokio v1.47.1 (*)
│   │   ├── tower-layer v0.3.3
│   │   ├── tower-service v0.3.3
│   │   └── tracing v0.1.41 (*)
│   ├── tower-layer v0.3.3
│   ├── tower-service v0.3.3
│   └── tracing v0.1.41 (*)
├── criterion v0.5.1
│   ├── anes v0.1.6
│   ├── cast v0.3.0
│   ├── ciborium v0.2.2
│   │   ├── ciborium-io v0.2.2
│   │   ├── ciborium-ll v0.2.2
│   │   │   ├── ciborium-io v0.2.2
│   │   │   └── half v2.6.0
│   │   │       └── cfg-if v1.0.3
│   │   └── serde v1.0.219 (*)
│   ├── clap v4.5.47 (*)
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
│   ├── regex v1.11.2 (*)
│   ├── serde v1.0.219 (*)
│   ├── serde_derive v1.0.219 (proc-macro) (*)
│   ├── serde_json v1.0.143 (*)
│   ├── tinytemplate v1.2.1
│   │   ├── serde v1.0.219 (*)
│   │   └── serde_json v1.0.143 (*)
│   └── walkdir v2.5.0 (*)
├── csv v1.3.1
│   ├── csv-core v0.1.12
│   │   └── memchr v2.7.5
│   ├── itoa v1.0.15
│   ├── ryu v1.0.20
│   └── serde v1.0.219 (*)
├── env_logger v0.11.8
│   ├── anstream v0.6.20 (*)
│   ├── anstyle v1.0.11
│   ├── env_filter v0.1.3
│   │   ├── log v0.4.28 (*)
│   │   └── regex v1.11.2 (*)
│   ├── jiff v0.2.15
│   └── log v0.4.28 (*)
├── insta v1.43.2
│   ├── console v0.15.11
│   │   ├── libc v0.2.175
│   │   └── once_cell v1.21.3
│   ├── globset v0.4.16
│   │   ├── aho-corasick v1.1.3 (*)
│   │   ├── bstr v1.12.0
│   │   │   └── memchr v2.7.5
│   │   ├── log v0.4.28 (*)
│   │   ├── regex-automata v0.4.10 (*)
│   │   └── regex-syntax v0.8.6
│   ├── once_cell v1.21.3
│   ├── similar v2.7.0
│   └── walkdir v2.5.0 (*)
├── jurisdiction v0.1.0 (/workspace/the-block/crates/jurisdiction) (*)
├── logtest v2.0.0
│   ├── lazy_static v1.5.0
│   └── log v0.4.28 (*)
├── proptest v1.7.0
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
│   │   ├── tempfile v3.22.0 (*)
│   │   └── wait-timeout v0.2.1
│   │       └── libc v0.2.175
│   ├── tempfile v3.22.0 (*)
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
│       ├── quote v1.0.40 (*)
│       └── syn v2.0.106 (*)
├── sha3 v0.10.8 (*)
├── tar v0.4.44 (*)
├── tracing v0.1.41 (*)
├── tracing-test v0.2.5
│   ├── tracing-core v0.1.34 (*)
│   ├── tracing-subscriber v0.3.20 (*)
│   └── tracing-test-macro v0.2.5 (proc-macro)
│       ├── quote v1.0.40 (*)
│       └── syn v2.0.106 (*)
└── wait-timeout v0.2.1 (*)
```
