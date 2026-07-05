//! Persistence metadata bằng SQLite (rusqlite, bundled) — hiện thực yêu cầu
//! "lưu vào cơ sở dữ liệu" cho quốc gia/ghi chú/thời gian khởi chạy/tài khoản.
//!
//! Bảng `instance_meta` khóa theo `vm_index`. Account lưu dạng JSON, được
//! **mã hóa AES-256-GCM** (tiền tố `enc:` + hex) khi có khóa — SEC-3 §9.
//! Khóa cùng nguồn với snapshot (`crypto::load_or_create_key`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::crypto::{self, Key32};
use crate::model::{AccountProfile, HardwareProfile, Profile, SnapshotMeta, SnapshotRecord};
use crate::state::InstanceMeta;

pub struct Db {
    conn: Mutex<Connection>,
    /// Khóa mã hóa account_json (credential). None = lưu plaintext (chỉ dùng cho test).
    enc_key: Option<Key32>,
}

impl Db {
    pub fn open_with_key(path: &Path, enc_key: Option<Key32>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS instance_meta (
                vm_index         INTEGER PRIMARY KEY,
                note             TEXT NOT NULL DEFAULT '',
                country          TEXT,
                last_launched_at INTEGER,
                account_json     TEXT,
                hardware_json    TEXT
            );
            CREATE TABLE IF NOT EXISTS snapshots (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                account_key  TEXT NOT NULL,
                storage_key  TEXT NOT NULL,
                sha256       TEXT NOT NULL,
                size_bytes   INTEGER NOT NULL,
                apk_version  TEXT,
                created_at   INTEGER NOT NULL,
                is_latest    INTEGER NOT NULL DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_snap_account ON snapshots(account_key);
            CREATE TABLE IF NOT EXISTS profiles (
                username      TEXT PRIMARY KEY,
                account_json  TEXT NOT NULL,
                hardware_json TEXT NOT NULL,
                country       TEXT,
                note          TEXT NOT NULL DEFAULT '',
                created_at    INTEGER NOT NULL,
                last_run_at   INTEGER
            );",
        )?;
        // Migration: thêm cột hardware_json cho DB tạo trước bản này (bỏ qua nếu đã có).
        let _ = conn.execute(
            "ALTER TABLE instance_meta ADD COLUMN hardware_json TEXT",
            [],
        );
        Ok(Self {
            conn: Mutex::new(conn),
            enc_key,
        })
    }

    /// Nạp toàn bộ metadata vào bộ nhớ lúc khởi động.
    pub fn load_all(&self) -> rusqlite::Result<HashMap<u32, InstanceMeta>> {
        let key = self.enc_key; // Copy → tránh mượn self trong closure
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT vm_index, note, country, last_launched_at, account_json, hardware_json FROM instance_meta",
        )?;
        let rows = stmt.query_map([], |row| {
            let index: u32 = row.get(0)?;
            let note: String = row.get(1)?;
            let country: Option<String> = row.get(2)?;
            let last_launched_at: Option<i64> = row.get(3)?;
            let account_json: Option<String> = row.get(4)?;
            let hardware_json: Option<String> = row.get(5)?;
            // Giải mã account_json nếu có tiền tố "enc:" (back-compat với bản plaintext cũ).
            let account = account_json.and_then(|s| {
                let json = match s.strip_prefix("enc:") {
                    Some(hex) => key
                        .as_ref()
                        .and_then(|k| crypto::decrypt_from_hex(k, hex).ok()),
                    None => Some(s),
                };
                json.and_then(|j| serde_json::from_str::<AccountProfile>(&j).ok())
            });
            let hardware =
                hardware_json.and_then(|s| serde_json::from_str::<HardwareProfile>(&s).ok());
            Ok((
                index,
                InstanceMeta {
                    account,
                    last_launched_at,
                    country,
                    note,
                    hardware,
                },
            ))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (i, m) = r?;
            map.insert(i, m);
        }
        Ok(map)
    }

    /// Ghi (thêm/cập nhật) metadata cho một VM.
    pub fn upsert(&self, index: u32, meta: &InstanceMeta) -> rusqlite::Result<()> {
        // Mã hóa credential trước khi ghi đĩa (SEC-3): thêm tiền tố "enc:" + hex.
        // Nếu không có khóa (test) hoặc mã hóa lỗi → lưu plaintext để không mất dữ liệu.
        let account_json = meta
            .account
            .as_ref()
            .and_then(|a| serde_json::to_string(a).ok())
            .map(|json| match self.enc_key.as_ref() {
                Some(k) => crypto::encrypt_to_hex(k, &json)
                    .map(|hex| format!("enc:{hex}"))
                    .unwrap_or(json),
                None => json,
            });
        let hardware_json = meta
            .hardware
            .as_ref()
            .and_then(|h| serde_json::to_string(h).ok());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO instance_meta (vm_index, note, country, last_launched_at, account_json, hardware_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(vm_index) DO UPDATE SET
                note=excluded.note,
                country=excluded.country,
                last_launched_at=excluded.last_launched_at,
                account_json=excluded.account_json,
                hardware_json=excluded.hardware_json",
            params![
                index,
                meta.note,
                meta.country,
                meta.last_launched_at,
                account_json,
                hardware_json
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, index: u32) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM instance_meta WHERE vm_index=?1",
            params![index],
        )?;
        Ok(())
    }

    // ── Profiles (kiến trúc disposable: profile là dữ liệu bền, tách khỏi vm_index) ──

    /// Ghi/cập nhật một profile (account mã hóa như instance_meta).
    pub fn upsert_profile(&self, p: &Profile) -> rusqlite::Result<()> {
        let account_json = serde_json::to_string(&p.account)
            .ok()
            .map(|json| match self.enc_key.as_ref() {
                Some(k) => crypto::encrypt_to_hex(k, &json)
                    .map(|hex| format!("enc:{hex}"))
                    .unwrap_or(json),
                None => json,
            })
            .unwrap_or_default();
        let hardware_json = serde_json::to_string(&p.hardware).unwrap_or_default();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO profiles (username, account_json, hardware_json, country, note, created_at, last_run_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(username) DO UPDATE SET
                account_json=excluded.account_json,
                hardware_json=excluded.hardware_json,
                country=excluded.country,
                note=excluded.note,
                last_run_at=excluded.last_run_at",
            params![
                p.username,
                account_json,
                hardware_json,
                p.country,
                p.note,
                p.created_at,
                p.last_run_at
            ],
        )?;
        Ok(())
    }

    /// Nạp toàn bộ profile (giải mã account). Bỏ qua bản ghi hỏng.
    pub fn load_profiles(&self) -> rusqlite::Result<Vec<Profile>> {
        let key = self.enc_key;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT username, account_json, hardware_json, country, note, created_at, last_run_at FROM profiles",
        )?;
        let rows = stmt.query_map([], |row| {
            let username: String = row.get(0)?;
            let account_json: String = row.get(1)?;
            let hardware_json: String = row.get(2)?;
            let country: Option<String> = row.get(3)?;
            let note: String = row.get(4)?;
            let created_at: i64 = row.get(5)?;
            let last_run_at: Option<i64> = row.get(6)?;
            let account_str = match account_json.strip_prefix("enc:") {
                Some(hex) => key
                    .as_ref()
                    .and_then(|k| crypto::decrypt_from_hex(k, hex).ok())
                    .unwrap_or_default(),
                None => account_json,
            };
            let account = serde_json::from_str::<AccountProfile>(&account_str).ok();
            let hardware = serde_json::from_str::<HardwareProfile>(&hardware_json).ok();
            Ok((
                username,
                account,
                hardware,
                country,
                note,
                created_at,
                last_run_at,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (username, account, hardware, country, note, created_at, last_run_at) = r?;
            if let (Some(account), Some(hardware)) = (account, hardware) {
                out.push(Profile {
                    username,
                    account,
                    hardware,
                    country,
                    note,
                    created_at,
                    last_run_at,
                });
            }
        }
        Ok(out)
    }

    pub fn delete_profile(&self, username: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM profiles WHERE username=?1", params![username])?;
        Ok(())
    }

    /// Ghi một snapshot mới cho `account_key` và đặt nó thành bản mới nhất.
    pub fn record_snapshot(
        &self,
        account_key: &str,
        storage_key: &str,
        meta: &SnapshotMeta,
        created_at: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE snapshots SET is_latest=0 WHERE account_key=?1",
            params![account_key],
        )?;
        conn.execute(
            "INSERT INTO snapshots
                (account_key, storage_key, sha256, size_bytes, apk_version, created_at, is_latest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            params![
                account_key,
                storage_key,
                meta.sha256,
                meta.size_bytes,
                meta.apk_version,
                created_at
            ],
        )?;
        Ok(())
    }

    /// storage_key của các snapshot CŨ vượt quá `keep` bản mới nhất (để xóa blob).
    pub fn snapshots_beyond(&self, account_key: &str, keep: u32) -> rusqlite::Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT storage_key FROM snapshots WHERE account_key=?1
             ORDER BY created_at DESC LIMIT -1 OFFSET ?2",
        )?;
        let keys = stmt
            .query_map(params![account_key, keep], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(keys)
    }

    /// Xóa bản ghi snapshot cũ, chỉ giữ `keep` bản mới nhất cho `account_key`.
    pub fn prune_snapshots(&self, account_key: &str, keep: u32) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM snapshots WHERE account_key=?1 AND id NOT IN (
                SELECT id FROM snapshots WHERE account_key=?1 ORDER BY created_at DESC LIMIT ?2
             )",
            params![account_key, keep],
        )?;
        Ok(())
    }

    /// Snapshot mới nhất của `account_key` (dùng để restore).
    pub fn latest_snapshot(&self, account_key: &str) -> rusqlite::Result<Option<SnapshotRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT storage_key, sha256, size_bytes, apk_version, created_at
             FROM snapshots WHERE account_key=?1 AND is_latest=1 LIMIT 1",
        )?;
        let mut rows = stmt.query(params![account_key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(SnapshotRecord {
                storage_key: row.get(0)?,
                sha256: row.get(1)?,
                size_bytes: row.get(2)?,
                apk_version: row.get(3)?,
                created_at: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InstanceMeta {
        InstanceMeta {
            account: None,
            last_launched_at: Some(1234),
            country: Some("VN".into()),
            note: "ghi chú test".into(),
            hardware: Some(HardwareProfile {
                model: "SM-G991B".into(),
                brand: "samsung".into(),
                manufacturer: "samsung".into(),
                imei: "356938035643809".into(),
                android_id: "a1b2c3d4e5f60718".into(),
                mac: "02:11:22:33:44:55".into(),
                res_width: 1080,
                res_height: 2400,
                dpi: 420,
                device: "o1s".into(),
                build_fingerprint:
                    "samsung/o1sxx/o1s:12/SP1A.210812.016/G991BXXU5CVF2:user/release-keys".into(),
            }),
        }
    }

    #[test]
    fn upsert_va_load_lai() {
        // DB in-memory cho test.
        let db = Db {
            conn: Mutex::new(Connection::open_in_memory().unwrap()),
            enc_key: None,
        };
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE instance_meta (vm_index INTEGER PRIMARY KEY, note TEXT NOT NULL DEFAULT '', country TEXT, last_launched_at INTEGER, account_json TEXT, hardware_json TEXT);",
            )
            .unwrap();

        db.upsert(3, &sample()).unwrap();
        let map = db.load_all().unwrap();
        assert_eq!(map.len(), 1);
        let m = &map[&3];
        assert_eq!(m.country.as_deref(), Some("VN"));
        assert_eq!(m.note, "ghi chú test");
        assert_eq!(m.last_launched_at, Some(1234));
        // Fingerprint được lưu & nạp lại đúng (yêu cầu: fingerprint trong CSDL).
        assert_eq!(m.hardware.as_ref().unwrap().imei, "356938035643809");
        assert_eq!(m.hardware.as_ref().unwrap().model, "SM-G991B");

        db.delete(3).unwrap();
        assert!(db.load_all().unwrap().is_empty());
    }

    #[test]
    fn account_json_duoc_ma_hoa_tren_dia() {
        // Có khóa → credential trong DB phải là ciphertext, không có mật khẩu trần.
        let db = Db {
            conn: Mutex::new(Connection::open_in_memory().unwrap()),
            enc_key: Some([42u8; 32]),
        };
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE instance_meta (vm_index INTEGER PRIMARY KEY, note TEXT NOT NULL DEFAULT '', country TEXT, last_launched_at INTEGER, account_json TEXT, hardware_json TEXT);",
            )
            .unwrap();

        let mut meta = sample();
        meta.account = Some(AccountProfile {
            tiktok_username: "tiktok_minh".into(),
            tiktok_password: "secret-pass".into(),
            two_fa: "".into(),
            tiktok_passkey: "".into(),
            email: "".into(),
            email_password: "".into(),
        });
        db.upsert(1, &meta).unwrap();

        // Đọc thô cột account_json: phải có tiền tố enc: và KHÔNG lộ mật khẩu.
        let raw: String = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT account_json FROM instance_meta WHERE vm_index=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(raw.starts_with("enc:"), "phải được mã hóa, thực tế: {raw}");
        assert!(!raw.contains("secret-pass"), "mật khẩu bị lộ trần!");

        // load_all giải mã lại đúng.
        let map = db.load_all().unwrap();
        let acc = map[&1].account.as_ref().unwrap();
        assert_eq!(acc.tiktok_password, "secret-pass");
    }

    #[test]
    fn snapshot_ghi_va_lay_ban_moi_nhat() {
        let path = std::env::temp_dir().join(format!("mpm_db_snap_{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open_with_key(&path, None).unwrap();

        let m1 = SnapshotMeta {
            sha256: "aaa".into(),
            size_bytes: 10,
            apk_version: "1.0".into(),
        };
        let m2 = SnapshotMeta {
            sha256: "bbb".into(),
            size_bytes: 20,
            apk_version: "1.1".into(),
        };
        db.record_snapshot("acc1", "acc1/1.tar", &m1, 100).unwrap();
        db.record_snapshot("acc1", "acc1/2.tar", &m2, 200).unwrap();

        // Bản mới nhất phải là m2, không phải m1.
        let latest = db.latest_snapshot("acc1").unwrap().unwrap();
        assert_eq!(latest.storage_key, "acc1/2.tar");
        assert_eq!(latest.sha256, "bbb");
        assert_eq!(latest.created_at, 200);

        assert!(db.latest_snapshot("khong-co").unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn profile_luu_nap_lai_va_ma_hoa() {
        let path = std::env::temp_dir().join(format!("mpm_db_prof_{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open_with_key(&path, Some([9u8; 32])).unwrap();
        let p = Profile {
            username: "acc1".into(),
            account: AccountProfile {
                tiktok_username: "acc1".into(),
                tiktok_password: "secret-pw".into(),
                two_fa: "".into(),
                tiktok_passkey: "".into(),
                email: "".into(),
                email_password: "".into(),
            },
            hardware: sample().hardware.unwrap(),
            country: Some("VN".into()),
            note: "ghi chú".into(),
            created_at: 100,
            last_run_at: None,
        };
        db.upsert_profile(&p).unwrap();

        // Nạp lại đúng + credential giải mã đúng.
        let loaded = db.load_profiles().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].username, "acc1");
        assert_eq!(loaded[0].account.tiktok_password, "secret-pw");
        assert_eq!(loaded[0].country.as_deref(), Some("VN"));

        // account_json trên đĩa PHẢI mã hóa.
        let raw: String = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT account_json FROM profiles WHERE username='acc1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(raw.starts_with("enc:"), "credential phải mã hóa");
        assert!(!raw.contains("secret-pw"));

        db.delete_profile("acc1").unwrap();
        assert!(db.load_profiles().unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retention_giu_n_ban_moi_nhat() {
        let path = std::env::temp_dir().join(format!("mpm_db_ret_{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open_with_key(&path, None).unwrap();

        for i in 0..7 {
            let m = SnapshotMeta {
                sha256: format!("h{i}"),
                size_bytes: 1,
                apk_version: "1".into(),
            };
            db.record_snapshot("acc", &format!("acc/{i}.tar"), &m, i as i64)
                .unwrap();
        }

        // Giữ 5 → 2 bản cũ nhất (created_at 0,1) nằm ngoài.
        let old = db.snapshots_beyond("acc", 5).unwrap();
        assert_eq!(old.len(), 2);
        assert!(old.contains(&"acc/0.tar".to_string()));
        assert!(old.contains(&"acc/1.tar".to_string()));

        db.prune_snapshots("acc", 5).unwrap();
        assert!(
            db.snapshots_beyond("acc", 0).unwrap().len() == 5,
            "chỉ còn 5 bản"
        );
        // Bản mới nhất vẫn là #6.
        assert_eq!(
            db.latest_snapshot("acc").unwrap().unwrap().storage_key,
            "acc/6.tar"
        );
        let _ = std::fs::remove_file(&path);
    }
}
