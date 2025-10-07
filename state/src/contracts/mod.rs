use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub type ContractId = u64;

struct Persisted {
    next_id: ContractId,
    code: HashMap<ContractId, Vec<u8>>,
    state: HashMap<ContractId, Vec<u8>>,
    wasm: HashMap<ContractId, Vec<u8>>,
    abi: HashMap<ContractId, Vec<u8>>,
}

impl Default for Persisted {
    fn default() -> Self {
        Self {
            next_id: 0,
            code: HashMap::new(),
            state: HashMap::new(),
            wasm: HashMap::new(),
            abi: HashMap::new(),
        }
    }
}

fn write_map(out: &mut Vec<u8>, map: &HashMap<ContractId, Vec<u8>>) {
    let mut keys: Vec<_> = map.keys().cloned().collect();
    keys.sort_unstable();
    out.extend_from_slice(&(keys.len() as u32).to_be_bytes());
    for key in keys {
        out.extend_from_slice(&key.to_be_bytes());
        if let Some(value) = map.get(&key) {
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
            out.extend_from_slice(value);
        }
    }
}

fn read_map(
    bytes: &[u8],
    cursor: &mut usize,
    context: &str,
) -> Result<HashMap<ContractId, Vec<u8>>, String> {
    let count = read_u32(bytes, cursor, format!("missing {context} count"))? as usize;
    let mut map = HashMap::with_capacity(count);
    for _ in 0..count {
        let id = read_u64(bytes, cursor, format!("missing {context} key"))?;
        let len = read_u32(bytes, cursor, format!("missing {context} value length"))? as usize;
        let value = read_bytes(bytes, cursor, len, format!("truncated {context} value"))?;
        map.insert(id, value.to_vec());
    }
    Ok(map)
}

fn read_u32(bytes: &[u8], cursor: &mut usize, message: String) -> Result<u32, String> {
    if bytes.len().saturating_sub(*cursor) < 4 {
        return Err(message);
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&bytes[*cursor..*cursor + 4]);
    *cursor += 4;
    Ok(u32::from_be_bytes(buf))
}

fn read_u64(bytes: &[u8], cursor: &mut usize, message: String) -> Result<u64, String> {
    if bytes.len().saturating_sub(*cursor) < 8 {
        return Err(message);
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[*cursor..*cursor + 8]);
    *cursor += 8;
    Ok(u64::from_be_bytes(buf))
}

fn read_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    message: String,
) -> Result<&'a [u8], String> {
    if bytes.len().saturating_sub(*cursor) < len {
        return Err(message);
    }
    let slice = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice)
}

impl Persisted {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.next_id.to_be_bytes());
        write_map(&mut out, &self.code);
        write_map(&mut out, &self.state);
        write_map(&mut out, &self.wasm);
        write_map(&mut out, &self.abi);
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut cursor = 0usize;
        let next_id = read_u64(bytes, &mut cursor, "missing next id".to_string())?;
        let code = read_map(bytes, &mut cursor, "code")?;
        let state = read_map(bytes, &mut cursor, "state")?;
        let wasm = read_map(bytes, &mut cursor, "wasm")?;
        let abi = read_map(bytes, &mut cursor, "abi")?;
        Ok(Self {
            next_id,
            code,
            state,
            wasm,
            abi,
        })
    }
}

/// Simple on-disk contract store using a compact first-party binary format.
pub struct ContractStore {
    path: Option<PathBuf>,
    data: Persisted,
    history: HashMap<ContractId, Vec<Vec<u8>>>,
}

impl ContractStore {
    /// Create a new store optionally backed by a file on disk.
    pub fn new(path: Option<PathBuf>) -> Self {
        let data = path
            .as_ref()
            .and_then(|p| fs::read(p).ok())
            .and_then(|b| Persisted::decode(&b).ok())
            .unwrap_or_default();
        Self {
            path,
            data,
            history: HashMap::new(),
        }
    }

    fn persist(&self) {
        if let Some(ref p) = self.path {
            let bytes = self.data.encode();
            if let Some(dir) = p.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(p, bytes);
        }
    }

    /// Deploy contract code returning a unique identifier.
    pub fn deploy(&mut self, code: Vec<u8>) -> ContractId {
        let id = self.data.next_id;
        self.data.next_id += 1;
        self.data.code.insert(id, code);
        self.persist();
        id
    }

    /// Deploy a WASM contract with an accompanying ABI descriptor.
    pub fn deploy_wasm(&mut self, wasm: Vec<u8>, abi: Vec<u8>) -> ContractId {
        let id = self.data.next_id;
        self.data.next_id += 1;
        self.data.wasm.insert(id, wasm);
        self.data.abi.insert(id, abi);
        self.persist();
        id
    }

    /// Fetch contract code.
    pub fn code(&self, id: ContractId) -> Option<&Vec<u8>> {
        self.data.code.get(&id)
    }

    /// Fetch WASM bytecode.
    pub fn wasm(&self, id: ContractId) -> Option<&Vec<u8>> {
        self.data.wasm.get(&id)
    }

    /// Fetch ABI descriptor bytes.
    pub fn abi(&self, id: ContractId) -> Option<&Vec<u8>> {
        self.data.abi.get(&id)
    }

    /// Fetch contract storage bytes.
    pub fn state(&self, id: ContractId) -> Option<Vec<u8>> {
        self.data.state.get(&id).cloned()
    }

    /// Overwrite contract storage bytes.
    pub fn set_state(&mut self, id: ContractId, data: Vec<u8>) {
        self.data.state.insert(id, data);
        self.persist();
    }

    /// Snapshot current storage bytes for debugging traces.
    pub fn snapshot_state(&mut self, id: ContractId) {
        let snap = self.state(id).unwrap_or_default();
        self.history.entry(id).or_default().push(snap);
    }

    /// Retrieve recorded state trace.
    pub fn trace(&self, id: ContractId) -> Option<&Vec<Vec<u8>>> {
        self.history.get(&id)
    }
}
