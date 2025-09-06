use std::sync::LazyLock;

const PREFIX: &[u8] = b"REMOTE_SIGN|"; // 12 bytes
static TAG: LazyLock<[u8; 16]> = LazyLock::new(|| {
    let mut buf = [0u8; 16];
    buf[..PREFIX.len()].copy_from_slice(PREFIX);
    buf
});

/// Prefix message with the remote signing domain tag.
pub fn remote_tag(msg: &[u8]) -> Vec<u8> {
    let mut out = TAG.to_vec();
    out.extend_from_slice(msg);
    out
}
