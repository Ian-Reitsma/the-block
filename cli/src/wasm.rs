#![forbid(unsafe_code)]

use the_block::vm::wasm;

/// Extract metadata for a first-party WASM module. Errors are surfaced as empty
/// payloads so callers can continue workflows that only depend on the sidecar
/// data.
pub fn extract_wasm_metadata(bytes: &[u8]) -> Vec<u8> {
    match wasm::analyze(bytes) {
        Ok(meta) => meta.encode(),
        Err(err) => {
            eprintln!("warning: failed to analyze wasm module: {err}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUSH_I64: u8 = 0x01;
    const RETURN: u8 = 0x10;

    fn sample_module() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&wasm::MAGIC);
        buf.push(wasm::VERSION_V1);
        buf.extend_from_slice(&[PUSH_I64, 1, 0, 0, 0, 0, 0, 0, 0, RETURN, 1]);
        buf
    }

    #[test]
    fn encodes_metadata_summary() {
        let module = sample_module();
        let encoded = extract_wasm_metadata(&module);
        let text = String::from_utf8(encoded).expect("utf8");
        assert!(text.contains("instructions=2"));
        assert!(text.contains("return_values=1"));
    }
}
