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
        // Additive migration keeps existing profiles and their fields intact.
        for table in ["groups", "accounts"] {
            let mut stmt = self
                .conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .map_err(|e| e.to_string())?;
            let columns = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            if !columns.iter().any(|name| name == "deleted") {
                self.conn
                    .execute_batch(&format!(
                        "ALTER TABLE {table} ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;"
                    ))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub fn list_groups(&self) -> Result<Vec<Group>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM groups WHERE deleted = 0 ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Group {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
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
            .execute(
                "UPDATE groups SET name = ?1 WHERE id = ?2",
                params![name, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_group(&self, id: i64) -> Result<(), String> {
        // Children stay attached; restoring the group does not revive accounts
        // that were separately trashed before the group was deleted.
        self.conn
            .execute("UPDATE groups SET deleted = 1 WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn trash_items(&self) -> Result<Vec<(bool, i64, String)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT 1, id, name FROM groups WHERE deleted = 1
             UNION ALL
             SELECT 0, a.id, a.site || ' · ' || g.name FROM accounts a
             JOIN groups g ON g.id = a.group_id WHERE a.deleted = 1 AND g.deleted = 0
             ORDER BY 1 DESC, 3",
            )
            .map_err(|e| e.to_string())?;
        let items = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?;
        items
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn restore(&self, group: bool, id: i64) -> Result<(), String> {
        let sql = if group {
            "UPDATE groups SET deleted = 0 WHERE id = ?1"
        } else {
            "UPDATE accounts SET deleted = 0 WHERE id = ?1"
        };
        self.conn
            .execute(sql, params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>, String> {
        let mut acc_stmt = self
            .conn
            .prepare(
                "SELECT a.id, a.group_id, a.site, a.pinned FROM accounts a
                 JOIN groups g ON g.id = a.group_id
                 WHERE a.deleted = 0 AND g.deleted = 0 ORDER BY a.pinned DESC, a.site",
            )
            .map_err(|e| e.to_string())?;
        let mut accounts: Vec<Account> = acc_stmt
            .query_map([], |r| {
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
            .execute("UPDATE accounts SET deleted = 1 WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Db {
        let db = Db::open(Path::new(":memory:"), None, None).unwrap();
        db.init_schema().unwrap();
        db
    }

    fn account(db: &Db, group_id: i64, site: &str) -> i64 {
        db.upsert_account(&Account {
            group_id,
            site: site.into(),
            pinned: true,
            fields: vec![Field {
                key: "email".into(),
                value: "user@example.com".into(),
            }],
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn global_listing_and_restore_preserve_fields_and_previous_deletions() {
        let db = database();
        let a = db.add_group("A").unwrap();
        let b = db.add_group("B").unwrap();
        let first = account(&db, a, "First");
        let second = account(&db, a, "Second");
        account(&db, b, "Third");
        assert_eq!(db.list_accounts().unwrap().len(), 3);
        db.delete_account(first).unwrap();
        db.delete_group(a).unwrap();
        assert_eq!(db.list_accounts().unwrap().len(), 1);
        assert_eq!(db.list_groups().unwrap().len(), 1);
        assert_eq!(db.trash_items().unwrap(), vec![(true, a, "A".into())]);
        db.restore(true, a).unwrap();
        let accounts = db.list_accounts().unwrap();
        assert_eq!(accounts.len(), 2);
        assert!(accounts.iter().any(|a| a.id == second));
        assert!(!accounts.iter().any(|a| a.id == first));
        assert_eq!(db.trash_items().unwrap()[0].1, first);
        db.restore(false, first).unwrap();
        let accounts = db.list_accounts().unwrap();
        let restored = accounts.iter().find(|a| a.id == first).unwrap();
        assert!(restored.pinned);
        assert_eq!(restored.group_id, a);
        assert_eq!(restored.fields[0].value, "user@example.com");
        assert!(db.trash_items().unwrap().is_empty());
    }

    #[test]
    fn legacy_schema_migration_is_repeatable_and_preserves_data() {
        let db = Db::open(Path::new(":memory:"), None, None).unwrap();
        db.conn.execute_batch("CREATE TABLE groups(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
            CREATE TABLE accounts(id INTEGER PRIMARY KEY, group_id INTEGER NOT NULL REFERENCES groups(id), site TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0);
            INSERT INTO groups VALUES(1, 'Legacy');
            INSERT INTO accounts VALUES(1, 1, 'Existing', 1);").unwrap();
        db.init_schema().unwrap();
        db.init_schema().unwrap();
        assert_eq!(db.list_accounts().unwrap()[0].site, "Existing");
        db.delete_group(1).unwrap();
        db.init_schema().unwrap();
        assert!(db.list_accounts().unwrap().is_empty());
        db.restore(true, 1).unwrap();
        assert_eq!(db.list_accounts().unwrap().len(), 1);
    }

    #[test]
    fn trash_survives_encrypted_profile_reopen() {
        let path = std::env::temp_dir().join(format!("am-trash-test-{}.am", rand::random::<u64>()));
        let key = [42u8; 32];
        let id;
        {
            let db = Db::open(&path, Some(&key), None).unwrap();
            db.init_schema().unwrap();
            let group = db.add_group("Encrypted").unwrap();
            id = account(&db, group, "Saved");
            db.delete_account(id).unwrap();
        }
        {
            let db = Db::open(&path, Some(&key), None).unwrap();
            db.init_schema().unwrap();
            assert!(db.list_accounts().unwrap().is_empty());
            assert_eq!(db.trash_items().unwrap()[0].1, id);
            db.restore(false, id).unwrap();
            assert_eq!(
                db.list_accounts().unwrap()[0].fields[0].value,
                "user@example.com"
            );
        }
        std::fs::remove_file(path).unwrap();
    }
}
