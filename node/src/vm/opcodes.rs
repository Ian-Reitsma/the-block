use serde::Serialize;

/// Core VM opcodes.
#[derive(Clone, Copy, Serialize, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    Halt = 0x00,
    /// Push a 64-bit immediate onto the stack.
    Push = 0x01,
    /// Pop two values, push their sum.
    Add = 0x02,
    /// Pop two values, push first minus second.
    Sub = 0x03,
    /// Pop two values, push their product.
    Mul = 0x04,
    /// Pop two values, push first divided by second (truncating).
    Div = 0x05,
    /// Pop two values, push bitwise AND.
    And = 0x06,
    /// Pop two values, push bitwise OR.
    Or = 0x07,
    /// Pop two values, push first modulo second.
    Mod = 0x08,
    /// Pop two values, push bitwise XOR.
    Xor = 0x09,
    /// Load value from contract storage and push onto stack.
    Load = 0x0a,
    /// Pop value and store to contract storage.
    Store = 0x0b,
    /// Pop value, push first 8 bytes of BLAKE3 hash.
    Hash = 0x0c,
}

impl OpCode {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::Halt),
            0x01 => Some(Self::Push),
            0x02 => Some(Self::Add),
            0x03 => Some(Self::Sub),
            0x04 => Some(Self::Mul),
            0x05 => Some(Self::Div),
            0x06 => Some(Self::And),
            0x07 => Some(Self::Or),
            0x08 => Some(Self::Mod),
            0x09 => Some(Self::Xor),
            0x0a => Some(Self::Load),
            0x0b => Some(Self::Store),
            0x0c => Some(Self::Hash),
            _ => None,
        }
    }

    pub fn all() -> &'static [OpCode] {
        &[
            OpCode::Halt,
            OpCode::Push,
            OpCode::Add,
            OpCode::Sub,
            OpCode::Mul,
            OpCode::Div,
            OpCode::And,
            OpCode::Or,
            OpCode::Mod,
            OpCode::Xor,
            OpCode::Load,
            OpCode::Store,
            OpCode::Hash,
        ]
    }
}

/// Write a JSON ABI mapping opcode names to discriminants.
pub fn write_abi(path: &std::path::Path) -> std::io::Result<()> {
    use foundation_serialization::json::{self, json};
    let mut map = json::Map::new();
    for op in OpCode::all() {
        map.insert(format!("{:?}", op).to_lowercase(), (*op as u8).into());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, json::to_vec_pretty(&json!(map))?)
}
