use std::path::Path;

use rusqlite::{params, Connection};
use zeroize::Zeroize;

use crate::crypto::key_to_hex;
use crate::model::{Account, Group};

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path, key: Option<&[u8]>) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        if let Some(k) = key {
            let mut hex = key_to_hex(k);
            let pragma = format!("PRAGMA key = \"x'{}'\";", hex);
            let result = conn.execute_batch(&pragma);
            hex.zeroize();
            result.map_err(|e| e.to_string())?;
        }

        // Trigger key verification: this fails if the key is wrong.
        conn.execute_batch("SELECT count(*) FROM sqlite_master;")
            .map_err(|_| "wrong password".to_string())?;

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
                    email TEXT NOT NULL DEFAULT '',
                    region TEXT NOT NULL DEFAULT '',
                    payment_methods TEXT NOT NULL DEFAULT '',
                    notes TEXT NOT NULL DEFAULT ''
                );",
            )
            .map_err(|e| e.to_string())
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
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, group_id, site, email, region, payment_methods, notes
                 FROM accounts WHERE group_id = ?1 ORDER BY site",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![group_id], |r| {
                Ok(Account {
                    id: r.get(0)?,
                    group_id: r.get(1)?,
                    site: r.get(2)?,
                    email: r.get(3)?,
                    region: r.get(4)?,
                    payment_methods: r.get(5)?,
                    notes: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn upsert_account(&self, a: &Account) -> Result<i64, String> {
        if a.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO accounts (group_id, site, email, region, payment_methods, notes)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![a.group_id, a.site, a.email, a.region, a.payment_methods, a.notes],
                )
                .map_err(|e| e.to_string())?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn
                .execute(
                    "UPDATE accounts
                     SET group_id=?1, site=?2, email=?3, region=?4, payment_methods=?5, notes=?6
                     WHERE id=?7",
                    params![
                        a.group_id,
                        a.site,
                        a.email,
                        a.region,
                        a.payment_methods,
                        a.notes,
                        a.id
                    ],
                )
                .map_err(|e| e.to_string())?;
            Ok(a.id)
        }
    }

    pub fn delete_account(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
