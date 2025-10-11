#![forbid(unsafe_code)]

use crate::Explorer;
use anyhow::Result;
use foundation_serialization::Serialize;
use foundation_sqlite::params;

#[derive(Serialize)]
pub struct SnarkProofRecord {
    pub job_id: String,
    pub verified: bool,
}

/// List recorded SNARK proof verification outcomes.
pub fn list_snark_proofs(exp: &Explorer) -> Result<Vec<SnarkProofRecord>> {
    let conn = exp.conn()?;
    let mut stmt = conn.prepare("SELECT job_id, verified FROM snark_proofs")?;
    let rows = stmt.query_map(params![], |row| {
        Ok(SnarkProofRecord {
            job_id: row.get(0)?,
            verified: row.get::<_, i64>(1)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}
