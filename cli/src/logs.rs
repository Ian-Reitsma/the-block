use clap::Subcommand;
use rusqlite::{params_from_iter, Connection, Row};

use base64::{engine::general_purpose, Engine as _};
use blake3::derive_key;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

#[derive(Subcommand, Debug)]
pub enum LogCmd {
    /// Search indexed logs stored in SQLite.
    Search {
        /// SQLite database produced by `log indexer`.
        db: String,
        /// Filter by peer identifier.
        #[arg(long)]
        peer: Option<String>,
        /// Filter by transaction hash correlation id.
        #[arg(long)]
        tx: Option<String>,
        /// Filter by block height.
        #[arg(long)]
        block: Option<u64>,
        /// Filter by raw correlation id value.
        #[arg(long)]
        correlation: Option<String>,
        /// Optional passphrase to decrypt encrypted log messages.
        #[arg(long)]
        passphrase: Option<String>,
        /// Maximum rows to return.
        #[arg(long)]
        limit: Option<usize>,
    },
}

pub fn handle(cmd: LogCmd) {
    match cmd {
        LogCmd::Search {
            db,
            peer,
            tx,
            block,
            correlation,
            passphrase,
            limit,
        } => {
            if let Err(e) = search(db, peer, tx, block, correlation, passphrase, limit) {
                eprintln!("log search failed: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn search(
    db: String,
    peer: Option<String>,
    tx: Option<String>,
    block: Option<u64>,
    correlation: Option<String>,
    passphrase: Option<String>,
    limit: Option<usize>,
) -> rusqlite::Result<()> {
    let conn = Connection::open(db)?;
    let mut clauses = Vec::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(peer) = peer {
        clauses.push("peer = ?".to_string());
        params.push(peer.into());
    }
    if let Some(tx) = tx {
        clauses.push("tx = ?".to_string());
        params.push(tx.into());
    }
    if let Some(block) = block {
        clauses.push("block = ?".to_string());
        params.push((block as i64).into());
    }
    if let Some(corr) = correlation {
        clauses.push("correlation_id = ?".to_string());
        params.push(corr.into());
    }
    let mut sql = String::from(
        "SELECT timestamp, level, message, correlation_id, peer, tx, block, encrypted, nonce FROM logs",
    );
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY timestamp DESC");
    if let Some(limit) = limit {
        sql.push_str(" LIMIT ?");
        params.push((limit as i64).into());
    }
    let mut stmt = conn.prepare(&sql)?;
    let key = passphrase.as_ref().map(|p| derive_key_bytes(p));
    let key_ref = key.as_ref();
    let mut rows = stmt.query(params_from_iter(params.iter()))?;
    while let Some(row) = rows.next()? {
        let entry = decode_row(row, key_ref)?;
        println!(
            "{} [{}] {} :: {}",
            entry.timestamp,
            entry.level,
            entry.correlation_id,
            entry.message
        );
    }
    Ok(())
}

struct QueryRow {
    timestamp: i64,
    level: String,
    message: String,
    correlation_id: String,
}

fn decode_row(row: &Row<'_>, key: Option<&Key<XChaCha20Poly1305>>) -> rusqlite::Result<QueryRow> {
    let encrypted: i64 = row.get("encrypted")?;
    let stored_msg: String = row.get("message")?;
    let nonce: Option<Vec<u8>> = row.get("nonce")?;
    let message = if encrypted == 1 {
        if let (Some(key), Some(nonce)) = (key, nonce.as_ref()) {
            decrypt_message(key, &stored_msg, nonce).unwrap_or_else(|| "<decrypt-failed>".into())
        } else {
            "<encrypted>".into()
        }
    } else {
        stored_msg
    };
    Ok(QueryRow {
        timestamp: row.get("timestamp")?,
        level: row.get("level")?,
        message,
        correlation_id: row.get("correlation_id")?,
    })
}

fn derive_key_bytes(passphrase: &str) -> Key<XChaCha20Poly1305> {
    let mut key_bytes = [0u8; 32];
    derive_key("the-block-log-indexer", passphrase.as_bytes(), &mut key_bytes);
    Key::from_slice(&key_bytes).to_owned()
}

fn decrypt_message(key: &Key<XChaCha20Poly1305>, data: &str, nonce: &[u8]) -> Option<String> {
    let cipher = XChaCha20Poly1305::new(key);
    let nonce = XNonce::from_slice(nonce);
    let bytes = general_purpose::STANDARD.decode(data).ok()?;
    cipher
        .decrypt(nonce, bytes.as_ref())
        .ok()
        .and_then(|plain| String::from_utf8(plain).ok())
}
