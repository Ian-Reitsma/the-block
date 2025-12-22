use foundation_sqlite::{params, Connection, Error as SqlError};
use std::env;
use std::path::Path;
use std::process;

fn main() {
    if let Err(err) = run() {
        eprintln!("explorer-migrate-treasury: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = env::args().nth(1).unwrap_or_else(|| "explorer.db".into());
    let db_path = Path::new(&path);
    if !db_path.exists() {
        return Err(format!("database not found at {}", db_path.display()));
    }
    let conn =
        Connection::open(db_path).map_err(|err| format!("open {}: {err}", db_path.display()))?;
    println!(
        "Applying treasury_disbursements migration to {}",
        db_path.display()
    );
    apply(
        &conn,
        "add status_payload column",
        "ALTER TABLE treasury_disbursements ADD COLUMN status_payload TEXT",
    );
    apply(
        &conn,
        "rename amount_ct to amount",
        "ALTER TABLE treasury_disbursements RENAME COLUMN amount_ct TO amount",
    );
    apply(
        &conn,
        "drop amount_it column",
        "ALTER TABLE treasury_disbursements DROP COLUMN amount_it",
    );
    println!("Migration checks complete.");
    Ok(())
}

fn apply(conn: &Connection, label: &str, sql: &str) {
    match conn.execute(sql, params![]) {
        Ok(_) => println!("  {label}: ok"),
        Err(err) if already_applied(&err) => {
            println!("  {label}: skipped ({err})");
        }
        Err(err) => {
            eprintln!("  {label}: {err}");
            process::exit(1);
        }
    }
}

fn already_applied(err: &SqlError) -> bool {
    let msg = err.to_string();
    msg.contains("duplicate column name")
        || msg.contains("no such column")
        || msg.contains("cannot drop column")
}
