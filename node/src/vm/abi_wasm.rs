use wasmtime::{AsContext, AsContextMut, Memory, Store};

/// Write bytes into WASM linear memory at offset 0.
#[must_use]
pub fn write(store: &mut Store<()>, memory: &Memory, data: &[u8]) -> i32 {
    let ptr = 0;
    let mem = memory.data_mut(store);
    if mem.len() >= data.len() {
        mem[..data.len()].copy_from_slice(data);
    }
    ptr
}

/// Read bytes from WASM linear memory.
#[must_use]
pub fn read(store: &Store<()>, memory: &Memory, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let mem = memory.data(store);
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    if end > mem.len() {
        return None;
    }
    Some(mem[start..end].to_vec())
}
