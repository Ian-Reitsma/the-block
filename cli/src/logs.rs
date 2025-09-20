use clap::Subcommand;

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
        /// Filter by severity level.
        #[arg(long)]
        level: Option<String>,
        /// Filter by minimum timestamp (inclusive).
        #[arg(long)]
        since: Option<u64>,
        /// Filter by maximum timestamp (inclusive).
        #[arg(long)]
        until: Option<u64>,
        /// Filter by internal row id greater than the provided value.
        #[arg(long = "after-id")]
        after_id: Option<u64>,
        /// Optional passphrase to decrypt encrypted log messages.
        #[arg(long)]
        passphrase: Option<String>,
        /// Maximum rows to return.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Re-encrypt stored messages with a new passphrase.
    RotateKey {
        /// SQLite database produced by `log indexer`.
        db: String,
        /// Existing passphrase protecting log messages.
        #[arg(long = "old-passphrase")]
        old_passphrase: Option<String>,
        /// New passphrase to apply.
        #[arg(long = "new-passphrase")]
        new_passphrase: Option<String>,
    },
    /// Fetch correlations from the metrics aggregator and stream matching logs.
    CorrelateMetric {
        /// Metrics aggregator base URL.
        #[arg(long, default_value = "http://localhost:9000")]
        aggregator: String,
        /// Metric name to correlate (e.g. quic_handshake_fail_total).
        #[arg(long)]
        metric: String,
        /// Optional override for the log database path.
        #[arg(long)]
        db: Option<String>,
        /// Maximum correlated metric entries to inspect.
        #[arg(long = "max-correlations", default_value_t = 1)]
        max_correlations: usize,
        /// Limit log rows returned per correlation.
        #[arg(long, default_value_t = 20)]
        rows: usize,
        /// Optional passphrase to decrypt log messages.
        #[arg(long)]
        passphrase: Option<String>,
    },
}

#[cfg(feature = "sqlite-storage")]
mod sqlite {
    use super::LogCmd;
    use anyhow::{anyhow, Result as AnyResult};
    use base64::{engine::general_purpose, Engine as _};
    use blake3::derive_key;
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
    use rand::rngs::OsRng;
    use rand::RngCore;
    use reqwest::blocking::Client as HttpClient;
    use rpassword::prompt_password;
    use rusqlite::{params, params_from_iter, Connection, Row};
    use serde::Deserialize;
    use std::env;

    #[derive(Debug, Deserialize)]
    struct AggregatorCorrelation {
        metric: String,
        correlation_id: String,
        peer_id: String,
        value: Option<f64>,
        timestamp: u64,
    }

    pub fn handle(cmd: LogCmd) {
        match cmd {
            LogCmd::Search {
                db,
                peer,
                tx,
                block,
                correlation,
                level,
                since,
                until,
                after_id,
                passphrase,
                limit,
            } => {
                let passphrase = prompt_optional_passphrase(
                    passphrase,
                    "Log passphrase (leave blank for none): ",
                );
                if let Err(e) = search(
                    db,
                    peer,
                    tx,
                    block,
                    correlation,
                    level,
                    since,
                    until,
                    after_id,
                    passphrase,
                    limit,
                ) {
                    eprintln!("log search failed: {e}");
                    std::process::exit(1);
                }
            }
            LogCmd::RotateKey {
                db,
                old_passphrase,
                new_passphrase,
            } => {
                let old = prompt_optional_passphrase(
                    old_passphrase,
                    "Current passphrase (leave blank for none): ",
                );
                let new_pass =
                    match prompt_optional_passphrase(new_passphrase, "New passphrase (required): ")
                    {
                        Some(p) => p,
                        None => {
                            eprintln!("new passphrase required");
                            std::process::exit(1);
                        }
                    };
                if let Err(e) = rotate_key(db, old, new_pass) {
                    eprintln!("rotate failed: {e}");
                    std::process::exit(1);
                }
            }
            LogCmd::CorrelateMetric {
                aggregator,
                metric,
                db,
                max_correlations,
                rows,
                passphrase,
            } => {
                let client = HttpClient::new();
                let url = format!(
                    "{}/correlations/{}",
                    aggregator.trim_end_matches('/'),
                    metric
                );
                let response = match client.get(&url).send() {
                    Ok(resp) => resp,
                    Err(err) => {
                        eprintln!("failed to query aggregator: {err}");
                        return;
                    }
                };
                if !response.status().is_success() {
                    eprintln!("aggregator responded with status {}", response.status());
                    return;
                }
                let mut records: Vec<AggregatorCorrelation> = match response.json() {
                    Ok(records) => records,
                    Err(err) => {
                        eprintln!("failed to decode aggregator response: {err}");
                        return;
                    }
                };
                if records.is_empty() {
                    eprintln!("no correlations recorded for metric {metric}");
                    return;
                }
                records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                let db_path = match db.or_else(|| env::var("TB_LOG_DB_PATH").ok()) {
                    Some(path) => path,
                    None => {
                        eprintln!("--db must be provided or TB_LOG_DB_PATH set");
                        return;
                    }
                };
                let passphrase = prompt_optional_passphrase(
                    passphrase,
                    "Log passphrase (leave blank for none): ",
                );
                let limit = max_correlations.max(1);
                for record in records.into_iter().take(limit) {
                    let value_str = record
                        .value
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "<none>".into());
                    println!(
                        "\nmetric={} correlation={} peer={} value={} timestamp={}",
                        record.metric,
                        record.correlation_id,
                        record.peer_id,
                        value_str,
                        record.timestamp
                    );
                    if let Err(err) = search(
                        db_path.clone(),
                        None,
                        None,
                        None,
                        Some(record.correlation_id.clone()),
                        None,
                        None,
                        None,
                        None,
                        passphrase.clone(),
                        Some(rows),
                    ) {
                        eprintln!("log search failed: {err}");
                    }
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
        level: Option<String>,
        since: Option<u64>,
        until: Option<u64>,
        after_id: Option<u64>,
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
        if let Some(level) = level {
            clauses.push("level = ?".to_string());
            params.push(level.into());
        }
        if let Some(since) = since {
            clauses.push("timestamp >= ?".to_string());
            params.push((since as i64).into());
        }
        if let Some(until) = until {
            clauses.push("timestamp <= ?".to_string());
            params.push((until as i64).into());
        }
        if let Some(after_id) = after_id {
            clauses.push("id > ?".to_string());
            params.push((after_id as i64).into());
        }
        let mut sql = String::from(
            "SELECT id, timestamp, level, message, correlation_id, peer, tx, block, encrypted, nonce FROM logs",
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
                "#{} {} [{}] {} :: {}",
                entry.id.unwrap_or(0),
                entry.timestamp,
                entry.level,
                entry.correlation_id,
                entry.message
            );
        }
        Ok(())
    }

    struct QueryRow {
        id: Option<u64>,
        timestamp: i64,
        level: String,
        message: String,
        correlation_id: String,
    }

    fn decode_row(row: &Row<'_>, key: Option<&Key>) -> rusqlite::Result<QueryRow> {
        let encrypted: i64 = row.get("encrypted")?;
        let stored_msg: String = row.get("message")?;
        let nonce: Option<Vec<u8>> = row.get("nonce")?;
        let message = if encrypted == 1 {
            if let (Some(key), Some(nonce)) = (key, nonce.as_ref()) {
                decrypt_message(key, &stored_msg, nonce)
                    .unwrap_or_else(|| "<decrypt-failed>".into())
            } else {
                "<encrypted>".into()
            }
        } else {
            stored_msg
        };
        Ok(QueryRow {
            id: row.get::<_, Option<i64>>("id")?.map(|v| v.max(0) as u64),
            timestamp: row.get("timestamp")?,
            level: row.get("level")?,
            message,
            correlation_id: row.get("correlation_id")?,
        })
    }

    fn rotate_key(db: String, current: Option<String>, new_pass: String) -> AnyResult<()> {
        let mut conn = Connection::open(db)?;
        let old_key = current.as_deref().map(derive_key_bytes);
        let new_key = derive_key_bytes(&new_pass);
        let tx = conn.transaction()?;
        let mut select_stmt = tx.prepare("SELECT id, message, nonce, encrypted FROM logs")?;
        let mut rows = select_stmt.query([])?;
        let mut updates = Vec::new();
        while let Some(row) = rows.next()? {
            let id: i64 = row.get("id")?;
            let nonce: Option<Vec<u8>> = row.get("nonce")?;
            let message: String = row.get("message")?;
            let encrypted_flag: i64 = row.get("encrypted")?;
            let plain = if encrypted_flag == 1 {
                let key = old_key
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing current passphrase"))?;
                let nonce_bytes = nonce.as_deref().ok_or_else(|| anyhow!("missing nonce"))?;
                decrypt_message(key, &message, nonce_bytes)
                    .ok_or_else(|| anyhow!("decrypt failed"))?
            } else {
                message.clone()
            };
            let (cipher, nonce_bytes) = encrypt_message(&new_key, &plain)?;
            updates.push((id, cipher, nonce_bytes));
        }
        drop(rows);
        drop(select_stmt);
        {
            let mut update_stmt = tx
                .prepare("UPDATE logs SET message = ?1, nonce = ?2, encrypted = 1 WHERE id = ?3")?;
            for (id, message, nonce) in updates {
                update_stmt.execute(params![message, nonce, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn derive_key_bytes(passphrase: &str) -> Key {
        let key_bytes = derive_key("the-block-log-indexer", passphrase.as_bytes());
        Key::from(key_bytes)
    }

    fn encrypt_message(key: &Key, message: &str) -> AnyResult<(String, Vec<u8>)> {
        let cipher = XChaCha20Poly1305::new(key);
        let mut nonce_bytes = [0u8; 24];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, message.as_bytes())
            .map_err(|e| anyhow!("encrypt: {e}"))?;
        Ok((
            general_purpose::STANDARD.encode(ciphertext),
            nonce_bytes.to_vec(),
        ))
    }

    fn decrypt_message(key: &Key, data: &str, nonce: &[u8]) -> Option<String> {
        let cipher = XChaCha20Poly1305::new(key);
        let nonce = XNonce::from_slice(nonce);
        let bytes = general_purpose::STANDARD.decode(data).ok()?;
        cipher
            .decrypt(nonce, bytes.as_ref())
            .ok()
            .and_then(|plain| String::from_utf8(plain).ok())
    }

    fn prompt_optional_passphrase(existing: Option<String>, prompt: &str) -> Option<String> {
        match existing {
            Some(p) => Some(p),
            None => prompt_password(prompt)
                .ok()
                .map(|input| input.trim().to_string())
                .filter(|s| !s.is_empty()),
        }
    }
}

#[cfg(feature = "sqlite-storage")]
pub use sqlite::handle;

#[cfg(not(feature = "sqlite-storage"))]
pub fn handle(cmd: LogCmd) {
    match cmd {
        LogCmd::Search { .. } | LogCmd::RotateKey { .. } | LogCmd::CorrelateMetric { .. } => {
            eprintln!(
                "log database commands require the `sqlite-storage` feature. Rebuild contract-cli with `--features sqlite-storage` or `--features full`.",
            );
            std::process::exit(1);
        }
    }
}
