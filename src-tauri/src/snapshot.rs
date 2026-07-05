//! Kho lưu archive snapshot (§ thiết kế Backup/Restore).
//!
//! Đảm bảo đồng thời 4 mục tiêu:
//! - **Tối ưu dung lượng**: nén zstd khi lưu, giải nén khi lấy.
//! - **Tốc độ**: zstd nhanh; nén/giải nén trong bộ nhớ.
//! - **Toàn vẹn**: ghi NGUYÊN TỬ (temp + rename) + sha256 của blob đã lưu.
//! - **Trừu tượng**: local (thư mục) ⇄ server (S3) không đổi call-site.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::crypto::{self, Key32};
use crate::error::AppResult;

/// Mức nén zstd. 19 cho tỉ lệ cao; dữ liệu phiên nhỏ nên vẫn nhanh.
const ZSTD_LEVEL: i32 = 19;

/// Metadata của blob ĐÃ LƯU (sau nén) — dùng để verify toàn vẹn & thống kê dung lượng.
#[derive(Debug, Clone)]
pub struct StoredMeta {
    pub sha256: String,
    pub size_bytes: u64,
}

/// Băm sha256 của một file → chuỗi hex thường.
pub fn sha256_file(path: &Path) -> AppResult<String> {
    Ok(sha256_bytes(&fs::read(path)?))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Nén + lưu NGUYÊN TỬ file vào kho theo `key`; trả về sha256/size của blob đã lưu.
    async fn put(&self, key: &str, file: &Path) -> AppResult<StoredMeta>;
    /// Giải nén archive theo `key` ra `dst` (khôi phục nội dung gốc).
    async fn get(&self, key: &str, dst: &Path) -> AppResult<()>;
    /// Kiểm tra sha256 của blob đã lưu (false nếu thiếu file).
    async fn verify(&self, key: &str, sha256: &str) -> AppResult<bool>;
    /// Xóa một blob (dùng cho retention).
    async fn delete(&self, key: &str) -> AppResult<()>;
}

/// Hiện thực local: mỗi snapshot là một file **nén rồi mã hóa** dưới `root/<key>`.
pub struct LocalSnapshotStore {
    root: PathBuf,
    /// Khóa AES-256-GCM. None = không mã hóa (chỉ nén).
    cipher_key: Option<Key32>,
    /// Bộ đếm để đặt tên file .tmp DUY NHẤT (chống đè khi hai `put` chạy song song).
    tmp_seq: AtomicU64,
}

impl LocalSnapshotStore {
    pub fn new(root: impl Into<PathBuf>, cipher_key: Option<Key32>) -> AppResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            cipher_key,
            tmp_seq: AtomicU64::new(0),
        })
    }

    fn key_path(&self, key: &str) -> PathBuf {
        // Phòng thủ TẦNG (username đã validate ở profile_ops; đây là lớp 2): chỉ ghép
        // các thành phần Normal — bỏ '..'/'.'/đường-dẫn-tuyệt-đối/ổ-đĩa — nên đường dẫn
        // kết quả LUÔN nằm trong `root`, không thể traversal ra ngoài.
        let mut p = self.root.clone();
        for comp in Path::new(key).components() {
            if let std::path::Component::Normal(seg) = comp {
                p.push(seg);
            }
        }
        p
    }
}

#[async_trait]
impl SnapshotStore for LocalSnapshotStore {
    async fn put(&self, key: &str, file: &Path) -> AppResult<StoredMeta> {
        let plain = fs::read(file)?;
        // Nén TRƯỚC rồi mã hóa (đúng thứ tự — mã hóa làm dữ liệu ngẫu nhiên, không nén được).
        let compressed = zstd::encode_all(&plain[..], ZSTD_LEVEL)
            .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
        let blob = match &self.cipher_key {
            Some(k) => crypto::encrypt(k, &compressed)?,
            None => compressed,
        };

        let dst = self.key_path(key);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        // Ghi nguyên tử: ghi .tmp DUY NHẤT rồi rename (rename cùng ổ đĩa là atomic) →
        // không bao giờ để lại snapshot ghi dở, và hai put song song không đè .tmp nhau.
        let seq = self.tmp_seq.fetch_add(1, Ordering::Relaxed);
        let fname = dst.file_name().and_then(|s| s.to_str()).unwrap_or("snap");
        let tmp = dst.with_file_name(format!("{fname}.{}.{seq}.tmp", std::process::id()));
        fs::write(&tmp, &blob)?;
        fs::rename(&tmp, &dst)?;

        Ok(StoredMeta {
            sha256: sha256_bytes(&blob),
            size_bytes: blob.len() as u64,
        })
    }

    async fn get(&self, key: &str, dst: &Path) -> AppResult<()> {
        let blob = fs::read(self.key_path(key))?;
        let compressed = match &self.cipher_key {
            Some(k) => crypto::decrypt(k, &blob)?,
            None => blob,
        };
        let plain = zstd::decode_all(&compressed[..])
            .map_err(|e| crate::error::AppError::Io(e.to_string()))?;
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dst, &plain)?;
        Ok(())
    }

    async fn verify(&self, key: &str, sha256: &str) -> AppResult<bool> {
        let path = self.key_path(key);
        if !path.exists() {
            return Ok(false);
        }
        Ok(sha256_file(&path)? == sha256)
    }

    async fn delete(&self, key: &str) -> AppResult<()> {
        let path = self.key_path(key);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mpm_snap_test_{}_{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn nen_giai_nen_giu_nguyen_va_giam_dung_luong() {
        let dir = tmp_dir("zstd");
        let store = LocalSnapshotStore::new(dir.join("kho"), Some([7u8; 32])).unwrap();

        // Dữ liệu dễ nén (lặp lại) → blob lưu phải nhỏ hơn nhiều.
        let plain = vec![b'A'; 100_000];
        let src = dir.join("nguon.tar");
        fs::write(&src, &plain).unwrap();

        let meta = store.put("acc/1.tar.zst", &src).await.unwrap();
        assert!(
            meta.size_bytes < 5_000,
            "phải nén nhỏ đi nhiều: {}",
            meta.size_bytes
        );
        assert_eq!(meta.sha256.len(), 64);

        // verify khớp sha của blob đã lưu.
        assert!(store.verify("acc/1.tar.zst", &meta.sha256).await.unwrap());
        assert!(!store.verify("acc/1.tar.zst", "sai").await.unwrap());

        // get giải nén ra đúng nội dung gốc.
        let out = dir.join("ra.tar");
        store.get("acc/1.tar.zst", &out).await.unwrap();
        assert_eq!(fs::read(&out).unwrap(), plain);

        // delete xóa blob.
        store.delete("acc/1.tar.zst").await.unwrap();
        assert!(!store.verify("acc/1.tar.zst", &meta.sha256).await.unwrap());

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn khong_de_lai_file_tmp_sau_khi_ghi() {
        let dir = tmp_dir("atomic");
        let store = LocalSnapshotStore::new(dir.join("kho"), Some([7u8; 32])).unwrap();
        let src = dir.join("s.tar");
        fs::write(&src, b"data").unwrap();
        store.put("a/x.tar.zst", &src).await.unwrap();
        // Không còn BẤT KỲ file .tmp nào trong thư mục kho sau khi ghi.
        let leftover: Vec<_> = fs::read_dir(dir.join("kho/a"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftover.is_empty(), "còn sót file .tmp: {leftover:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
