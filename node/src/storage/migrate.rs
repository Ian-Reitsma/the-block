//! Database schema migrations.

use std::convert::TryInto;

use sled::Db;

const SCHEMA_VERSION_KEY: &[u8] = b"schema_version";
const LATEST: u32 = 1;

fn current_version(db: &Db) -> u32 {
    db.get(SCHEMA_VERSION_KEY)
        .ok()
        .flatten()
        .and_then(|v| {
            let bytes: [u8; 4] = v.as_slice().try_into().ok()?;
            Some(u32::from_le_bytes(bytes))
        })
        .unwrap_or(0)
}

fn set_version(db: &Db, v: u32) {
    let _ = db.insert(SCHEMA_VERSION_KEY, v.to_le_bytes().to_vec());
}

pub fn migrate(db: &Db) -> sled::Result<()> {
    let mut v = current_version(db);
    while v < LATEST {
        match v {
            0 => migrate_v0_to_v1(db)?,
            _ => break,
        }
        v = current_version(db);
    }
    Ok(())
}

fn migrate_v0_to_v1(db: &Db) -> sled::Result<()> {
    // future migration work; currently just bumps the version marker
    set_version(db, 1);
    db.flush()?;
    Ok(())
}
