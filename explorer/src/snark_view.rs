#![forbid(unsafe_code)]

use crate::Explorer;
use diagnostics::anyhow::{self, Result};
use foundation_serialization::Serialize;
use foundation_sqlite::params;

#[derive(Serialize)]
pub struct SnarkProofRecord {
    pub job_id: String,
    pub verified: bool,
}

/// List recorded SNARK proof verification outcomes.
pub fn list_snark_proofs(exp: &Explorer) -> Result<Vec<SnarkProofRecord>> {
    let conn = exp.conn().map_err(anyhow::Error::from_error)?;
    let mut stmt = conn
        .prepare("SELECT job_id, verified FROM snark_proofs")
        .map_err(anyhow::Error::from_error)?;
    let rows = stmt
        .query_map(params![], |row| {
            Ok(SnarkProofRecord {
                job_id: row.get(0)?,
                verified: row.get::<_, i64>(1)? != 0,
            })
        })
        .map_err(anyhow::Error::from_error)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(anyhow::Error::from_error)?);
    }
    Ok(out)
}
