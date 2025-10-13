#![forbid(unsafe_code)]

use crate::{Explorer, TxRecord};
use diagnostics::anyhow::{self, Result};

/// List HTLC related transactions from the explorer database.
pub fn list_htlcs(exp: &Explorer) -> Result<Vec<TxRecord>> {
    let conn = exp.conn().map_err(anyhow::Error::from_error)?;
    let mut stmt = conn
        .prepare("SELECT hash, block_hash, memo, contract, data FROM txs WHERE contract='htlc'")
        .map_err(anyhow::Error::from_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TxRecord {
                hash: row.get(0)?,
                block_hash: row.get(1)?,
                memo: row.get(2)?,
                contract: row.get(3)?,
                data: row.get(4)?,
            })
        })
        .map_err(anyhow::Error::from_error)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(anyhow::Error::from_error)?);
    }
    Ok(out)
}
