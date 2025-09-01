use serde::Serialize;

/// Core VM opcodes.
#[derive(Clone, Copy, Serialize, Debug)]
#[repr(u8)]
pub enum OpCode {
    Halt = 0x00,
    /// Push a 64-bit immediate onto the stack.
    Push = 0x01,
    /// Pop two values, push their sum.
    Add = 0x02,
    /// Pop two values, push first minus second.
    Sub = 0x03,
}

impl OpCode {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Halt),
            0x01 => Some(Self::Push),
            0x02 => Some(Self::Add),
            0x03 => Some(Self::Sub),
            _ => None,
        }
    }

    pub fn all() -> &'static [OpCode] {
        &[OpCode::Halt, OpCode::Push, OpCode::Add, OpCode::Sub]
    }
}

/// Write a JSON ABI mapping opcode names to discriminants.
pub fn write_abi(path: &std::path::Path) -> std::io::Result<()> {
    use serde_json::json;
    let mut map = serde_json::Map::new();
    for op in OpCode::all() {
        map.insert(format!("{:?}", op).to_lowercase(), (*op as u8).into());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_vec_pretty(&json!(map))?)
}
