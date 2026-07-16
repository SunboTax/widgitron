use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const STATE_DB_BUSY_TIMEOUT: Duration = Duration::from_millis(250);

/// Read a small set of text values directly from a VS Code-family state.vscdb.
///
/// The IDE keeps this SQLite database open, but read-only WAL readers can safely
/// coexist with it. Copying the whole database for a handful of keys is both
/// unnecessary and potentially catastrophic when state.vscdb has grown to many
/// gigabytes.
pub fn read_text_keys(db_path: &Path, keys: &[&str]) -> Result<HashMap<String, String>, String> {
    if !db_path.exists() {
        return Ok(HashMap::new());
    }

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db_path, flags)
        .map_err(|error| format!("Failed to open state database read-only: {}", error))?;
    conn.busy_timeout(STATE_DB_BUSY_TIMEOUT)
        .map_err(|error| format!("Failed to set state database busy timeout: {}", error))?;

    let mut statement = conn
        .prepare("SELECT value FROM ItemTable WHERE key = ?1")
        .map_err(|error| format!("Failed to prepare state database query: {}", error))?;
    let mut values = HashMap::with_capacity(keys.len());

    for key in keys {
        let value = statement
            .query_row([key], |row| row.get::<_, String>(0))
            .optional()
            .map_err(|error| format!("Failed to read state database key '{}': {}", key, error))?;
        if let Some(value) = value {
            values.insert((*key).to_string(), value);
        }
    }

    Ok(values)
}

pub fn read_text_key(db_path: &Path, key: &str) -> Result<Option<String>, String> {
    Ok(read_text_keys(db_path, &[key])?.remove(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_requested_keys_from_a_live_read_only_database() {
        let path = std::env::temp_dir().join(format!(
            "widgitron_state_db_test_{}_{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let conn = Connection::open(&path).expect("create state database");
        conn.execute(
            "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .expect("create ItemTable");
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            ["cursorAuth/accessToken", "token-value"],
        )
        .expect("insert token");
        drop(conn);

        let values = read_text_keys(&path, &["cursorAuth/accessToken", "cursorAuth/cachedEmail"])
            .expect("read state keys");

        assert_eq!(
            values.get("cursorAuth/accessToken").map(String::as_str),
            Some("token-value")
        );
        assert!(!values.contains_key("cursorAuth/cachedEmail"));

        let _ = std::fs::remove_file(path);
    }
}
