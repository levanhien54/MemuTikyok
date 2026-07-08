//! Trích **resetprop** từ Magisk APK để khóa `ro.*` (model/fingerprint) trên VM MuMu.
//!
//! Bối cảnh: MuMu có root NATIVE (`enable_su=1` → uid=0) nhưng KHÔNG có Magisk/resetprop.
//! `resetprop` là applet của binary `magisk` (đóng gói sẵn trong Magisk APK dưới
//! `lib/<abi>/libmagisk.so`). Vì đã có root, ta chỉ cần **đẩy binary vào VM và chạy
//! `magisk resetprop`** — KHÔNG cần cài Magisk vào hệ thống. Hợp với mô hình disposable:
//! provision đẩy binary vào mỗi VM mới rồi `lock_device_identity` dùng nó (kiểm chứng thực:
//! `magisk -c` = 30.7, resetprop đổi được brand/manufacturer/build.fingerprint).

use std::io::Read;
use std::path::{Path, PathBuf};

/// Đường dẫn binary magisk trong VM (đẩy vào /data/local/tmp; VM disposable nên đẩy mỗi run).
pub const VM_MAGISK_PATH: &str = "/data/local/tmp/magisk";

/// Trích `lib/<abi>/libmagisk.so` từ Magisk APK ra `cache_dir/magisk-<abi>` (idempotent —
/// đã có thì trả luôn). MuMu hiện tại là **x86_64** (đã kiểm chứng); hỗ trợ thêm x86.
/// Trả `None` nếu apk không mở được / thiếu entry.
pub fn ensure_binary(apk_path: &str, cache_dir: &Path) -> Option<PathBuf> {
    // MuMu android 9 = x86_64 (abilist: x86_64,arm64-v8a,x86,...). Dùng x86_64.
    ensure_for_abi(apk_path, cache_dir, "x86_64")
        .or_else(|| ensure_for_abi(apk_path, cache_dir, "x86"))
}

/// Trần dung lượng ĐẶT TRƯỚC khi trích (libmagisk.so thật ~0.5MB) — không tin size
/// khai báo trong zip (APK hỏng/bịa có thể khai size khổng lồ → cấp phát phi lý).
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

fn ensure_for_abi(apk_path: &str, cache_dir: &Path, abi: &str) -> Option<PathBuf> {
    let out = cache_dir.join(format!("magisk-{abi}"));
    let apk_meta = std::fs::metadata(apk_path).ok()?;
    // Cache hợp lệ khi: tồn tại + không rỗng + KHÔNG CŨ HƠN APK nguồn (mtime). Đổi/nâng cấp
    // Magisk APK (mtime mới hơn) → trích lại, không dùng binary cũ (finding: cache-by-ABI cũ).
    if let Ok(out_meta) = std::fs::metadata(&out) {
        let non_empty = out_meta.len() > 0;
        let fresh = match (out_meta.modified(), apk_meta.modified()) {
            (Ok(o), Ok(a)) => o >= a,
            _ => true, // không đọc được mtime → giữ hành vi cũ (coi là hợp lệ)
        };
        if non_empty && fresh {
            return Some(out);
        }
    }
    let file = std::fs::File::open(apk_path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let entry_name = format!("lib/{abi}/libmagisk.so");
    let mut entry = zip.by_name(&entry_name).ok()?;
    let cap = entry.size().min(MAX_ENTRY_BYTES) as usize;
    let mut buf = Vec::with_capacity(cap);
    entry.read_to_end(&mut buf).ok()?;
    if buf.is_empty() {
        return None;
    }
    std::fs::create_dir_all(cache_dir).ok()?;
    // Ghi NGUYÊN TỬ: ghi tmp rồi rename. Crash giữa chừng để lại tmp (không phải file đích dở)
    // → lần sau không tưởng nhầm binary trích DỞ (len>0) là hợp lệ. rename thay thế file cũ.
    let tmp = cache_dir.join(format!("magisk-{abi}.tmp"));
    std::fs::write(&tmp, &buf).ok()?;
    std::fs::rename(&tmp, &out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trích thật từ Magisk APK người dùng đã tải (bỏ qua nếu chưa có file). Xác minh
    /// binary trích ra là ELF (magic \x7fELF) và không rỗng.
    #[test]
    fn trich_libmagisk_tu_apk_that() {
        let apk = r"D:\MemuTiktok\appTiktok\Magisk-v30.7.apk";
        if !Path::new(apk).is_file() {
            eprintln!("bỏ qua: chưa có {apk}");
            return;
        }
        let tmp = std::env::temp_dir().join("mpm-magisk-test");
        let _ = std::fs::remove_dir_all(&tmp);
        let bin = ensure_binary(apk, &tmp).expect("phải trích được libmagisk.so");
        let bytes = std::fs::read(&bin).unwrap();
        assert!(bytes.len() > 100_000, "binary quá nhỏ: {}", bytes.len());
        assert_eq!(&bytes[..4], b"\x7fELF", "không phải ELF");
        // Idempotent: gọi lần 2 trả cùng đường dẫn, không trích lại.
        assert_eq!(ensure_binary(apk, &tmp).as_deref(), Some(bin.as_path()));
        // Cache RỖNG (trích dở do crash) → KHÔNG được tái dùng, phải trích lại binary thật.
        std::fs::write(&bin, b"").unwrap();
        let bin2 = ensure_binary(apk, &tmp).expect("trích lại khi cache rỗng");
        assert!(
            std::fs::metadata(&bin2).unwrap().len() > 100_000,
            "cache rỗng phải được trích lại, không trả file rỗng"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
