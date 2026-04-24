use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection};
use zeroize::Zeroize;

use crate::crypto::key_to_hex;
use crate::model::{Account, Field, Group};


pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path, key: Option<&[u8]>, salt: Option<&[u8]>) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        if let Some(k) = key {
            let mut hex = key_to_hex(k);
            let pragma = format!("PRAGMA key = \"x'{}'\";", hex);
            let result = conn.execute_batch(&pragma);
            hex.zeroize();
            result.map_err(|e| e.to_string())?;

            if let Some(s) = salt {
                // cipher_salt must be set AFTER key for SQLCipher to honor it
                // when creating a new database. This pins the file's first 16
                // bytes to our Argon2id salt, so re-opens can re-derive the key.
                let mut salt_hex = key_to_hex(s);
                let pragma = format!("PRAGMA cipher_salt = \"x'{}'\";", salt_hex);
                let result = conn.execute_batch(&pragma);
                salt_hex.zeroize();
                result.map_err(|e| e.to_string())?;
            }
        }

        // Trigger key verification: this fails if the key is wrong.
        conn.execute_batch("SELECT count(*) FROM sqlite_master;")
            .map_err(|_| "wrong password".to_string())?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| e.to_string())?;

        Ok(Self { conn })
    }

    pub fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS groups (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE
                );
                CREATE TABLE IF NOT EXISTS accounts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
                    site TEXT NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS account_fields (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                    position INTEGER NOT NULL DEFAULT 0,
                    key TEXT NOT NULL,
                    value TEXT NOT NULL DEFAULT ''
                );
                CREATE INDEX IF NOT EXISTS idx_account_fields_account
                    ON account_fields(account_id);
                CREATE TABLE IF NOT EXISTS prefs (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );",
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_groups(&self) -> Result<Vec<Group>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM groups ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Group {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn add_group(&self, name: &str) -> Result<i64, String> {
        self.conn
            .execute("INSERT INTO groups (name) VALUES (?1)", params![name])
            .map_err(|e| e.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn load_prefs(&self) -> Result<HashMap<String, String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM prefs")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<HashMap<_, _>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn set_pref(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO prefs (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn rekey(&self, key: &[u8]) -> Result<(), String> {
        let mut hex = key_to_hex(key);
        let pragma = format!("PRAGMA rekey = \"x'{}'\";", hex);
        let result = self.conn.execute_batch(&pragma);
        hex.zeroize();
        result.map_err(|e| e.to_string())
    }

    pub fn rename_group(&self, id: i64, name: &str) -> Result<(), String> {
        self.conn
            .execute("UPDATE groups SET name = ?1 WHERE id = ?2", params![name, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_group(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM accounts WHERE group_id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM groups WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_accounts(&self, group_id: i64) -> Result<Vec<Account>, String> {
        let mut acc_stmt = self
            .conn
            .prepare(
                "SELECT id, group_id, site, pinned FROM accounts
                 WHERE group_id = ?1 ORDER BY pinned DESC, site",
            )
            .map_err(|e| e.to_string())?;
        let mut accounts: Vec<Account> = acc_stmt
            .query_map(params![group_id], |r| {
                let pinned: i64 = r.get(3)?;
                Ok(Account {
                    id: r.get(0)?,
                    group_id: r.get(1)?,
                    site: r.get(2)?,
                    pinned: pinned != 0,
                    fields: Vec::new(),
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let mut field_stmt = self
            .conn
            .prepare(
                "SELECT key, value FROM account_fields
                 WHERE account_id = ?1 ORDER BY position, id",
            )
            .map_err(|e| e.to_string())?;

        for a in &mut accounts {
            let rows = field_stmt
                .query_map(params![a.id], |r| {
                    Ok(Field {
                        key: r.get(0)?,
                        value: r.get(1)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            a.fields = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
        }

        Ok(accounts)
    }

    pub fn upsert_account(&self, a: &Account) -> Result<i64, String> {
        let pinned = if a.pinned { 1i64 } else { 0 };
        let id = if a.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO accounts (group_id, site, pinned) VALUES (?1, ?2, ?3)",
                    params![a.group_id, a.site, pinned],
                )
                .map_err(|e| e.to_string())?;
            self.conn.last_insert_rowid()
        } else {
            self.conn
                .execute(
                    "UPDATE accounts SET group_id = ?1, site = ?2, pinned = ?3 WHERE id = ?4",
                    params![a.group_id, a.site, pinned, a.id],
                )
                .map_err(|e| e.to_string())?;
            a.id
        };

        self.conn
            .execute(
                "DELETE FROM account_fields WHERE account_id = ?1",
                params![id],
            )
            .map_err(|e| e.to_string())?;

        for (i, f) in a.fields.iter().enumerate() {
            if f.key.trim().is_empty() && f.value.trim().is_empty() {
                continue;
            }
            self.conn
                .execute(
                    "INSERT INTO account_fields (account_id, position, key, value)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, i as i64, f.key, f.value],
                )
                .map_err(|e| e.to_string())?;
        }

        Ok(id)
    }

    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE accounts SET pinned = ?1 WHERE id = ?2",
                params![if pinned { 1i64 } else { 0 }, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_account(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
