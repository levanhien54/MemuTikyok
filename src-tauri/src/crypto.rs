//! Mã hóa dữ liệu nhạy cảm lúc nghỉ (SEC-3 §9 SRS). Dùng AES-256-GCM (AEAD):
//! vừa **bảo mật** vừa **toàn vẹn** (tag xác thực — sửa 1 byte là giải mã thất bại).
//!
//! Định dạng blob: `nonce(12) || ciphertext+tag`.
//!
//! Khóa lưu ở `snapshot.key` trong thư mục dữ liệu, **bọc bằng Windows DPAPI**
//! (`CryptProtectData` — gắn với tài khoản Windows) nên không nằm trần trên đĩa.
//! Khóa này dùng chung cho cả snapshot lẫn `account_json` (credential) trong DB.

use std::fs;
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};

use crate::error::{AppError, AppResult};

pub type Key32 = [u8; 32];

fn rand_bytes(buf: &mut [u8]) -> AppResult<()> {
    getrandom::getrandom(buf).map_err(|e| AppError::Io(format!("RNG lỗi: {e}")))
}

/// Nạp khóa từ file, hoặc sinh mới (32 byte ngẫu nhiên) và lưu nếu chưa có.
///
/// Khóa được **bọc bằng Windows DPAPI** (`CryptProtectData`, gắn với tài khoản
/// Windows hiện tại) trước khi ghi đĩa — SEC-3. Nếu file cũ là khóa trần 32 byte
/// (bản trước), nó vẫn được nhận và **tự nâng cấp** lên dạng DPAPI ở lần ghi này.
pub fn load_or_create_key(path: &Path) -> AppResult<Key32> {
    if let Ok(bytes) = fs::read(path) {
        // Ưu tiên: file là blob DPAPI → gỡ bọc ra 32 byte.
        if let Some(plain) = dpapi_unprotect(&bytes) {
            if plain.len() == 32 {
                let mut k = [0u8; 32];
                k.copy_from_slice(&plain);
                return Ok(k);
            }
        }
        // Back-compat: file khóa trần 32 byte (bản cũ) → nhận & nâng cấp lên DPAPI.
        if bytes.len() == 32 {
            let mut k = [0u8; 32];
            k.copy_from_slice(&bytes);
            if let Some(wrapped) = dpapi_protect(&k) {
                let _ = fs::write(path, &wrapped); // best-effort nâng cấp
            }
            return Ok(k);
        }
    }
    let mut k = [0u8; 32];
    rand_bytes(&mut k)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Bọc DPAPI nếu được; nếu không (không phải Windows / lỗi) thì ghi trần.
    let to_write = dpapi_protect(&k).unwrap_or_else(|| k.to_vec());
    fs::write(path, to_write)?;
    Ok(k)
}

// ── Windows DPAPI (CryptProtectData/Unprotect) qua FFI trực tiếp, không thêm crate ──
#[cfg(windows)]
mod dpapi {
    use core::ffi::c_void;

    #[repr(C)]
    struct DataBlob {
        cb: u32,
        pb: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            data_in: *const DataBlob,
            desc: *const u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            data_in: *const DataBlob,
            desc: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(mem: *mut c_void) -> *mut c_void;
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    fn call(input: &[u8], protect: bool) -> Option<Vec<u8>> {
        unsafe {
            let in_blob = DataBlob {
                cb: input.len() as u32,
                pb: input.as_ptr() as *mut u8,
            };
            let mut out = DataBlob {
                cb: 0,
                pb: core::ptr::null_mut(),
            };
            let ok = if protect {
                CryptProtectData(
                    &in_blob,
                    core::ptr::null(),
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            } else {
                CryptUnprotectData(
                    &in_blob,
                    core::ptr::null_mut(),
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            };
            if ok == 0 || out.pb.is_null() {
                return None;
            }
            let v = core::slice::from_raw_parts(out.pb, out.cb as usize).to_vec();
            LocalFree(out.pb as *mut c_void);
            Some(v)
        }
    }

    pub fn protect(d: &[u8]) -> Option<Vec<u8>> {
        call(d, true)
    }
    pub fn unprotect(d: &[u8]) -> Option<Vec<u8>> {
        call(d, false)
    }
}

#[cfg(windows)]
fn dpapi_protect(d: &[u8]) -> Option<Vec<u8>> {
    dpapi::protect(d)
}
#[cfg(windows)]
fn dpapi_unprotect(d: &[u8]) -> Option<Vec<u8>> {
    dpapi::unprotect(d)
}
#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Option<Vec<u8>> {
    None
}
#[cfg(not(windows))]
fn dpapi_unprotect(_: &[u8]) -> Option<Vec<u8>> {
    None
}

pub fn encrypt(key: &Key32, plain: &[u8]) -> AppResult<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; 12];
    rand_bytes(&mut nonce)?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|_| AppError::Io("mã hóa thất bại".into()))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> AppResult<Vec<u8>> {
    if s.len() % 2 != 0 {
        return Err(AppError::Io("chuỗi hex độ dài lẻ".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| AppError::Io("hex sai".into())))
        .collect()
}

/// Mã hóa chuỗi → hex (để lưu vào cột TEXT của SQLite).
pub fn encrypt_to_hex(key: &Key32, plain: &str) -> AppResult<String> {
    Ok(hex_encode(&encrypt(key, plain.as_bytes())?))
}

/// Giải mã chuỗi hex → plaintext.
pub fn decrypt_from_hex(key: &Key32, hex: &str) -> AppResult<String> {
    let plain = decrypt(key, &hex_decode(hex)?)?;
    String::from_utf8(plain).map_err(|_| AppError::Io("giải mã ra không phải UTF-8".into()))
}

pub fn decrypt(key: &Key32, blob: &[u8]) -> AppResult<Vec<u8>> {
    if blob.len() < 12 {
        return Err(AppError::Io("blob mã hóa không hợp lệ".into()));
    }
    let (nonce, ct) = blob.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| AppError::Io("giải mã thất bại (khóa sai hoặc dữ liệu bị sửa)".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ma_hoa_giai_ma_vong_tron() {
        let key = [7u8; 32];
        let msg = b"session-cookie-tiktok-bi-mat";
        let blob = encrypt(&key, msg).unwrap();
        assert_ne!(&blob[12..], &msg[..], "ciphertext phải khác plaintext");
        assert_eq!(decrypt(&key, &blob).unwrap(), msg);
    }

    #[test]
    fn khoa_sai_thi_that_bai() {
        let blob = encrypt(&[1u8; 32], b"x").unwrap();
        assert!(decrypt(&[2u8; 32], &blob).is_err());
    }

    #[test]
    fn du_lieu_bi_sua_thi_that_bai() {
        let key = [3u8; 32];
        let mut blob = encrypt(&key, b"hello world").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xff; // lật 1 byte trong tag/ciphertext
        assert!(decrypt(&key, &blob).is_err(), "GCM phải phát hiện sửa đổi");
    }

    #[test]
    fn nonce_ngau_nhien_moi_lan() {
        let key = [9u8; 32];
        let a = encrypt(&key, b"same").unwrap();
        let b = encrypt(&key, b"same").unwrap();
        assert_ne!(a, b, "cùng plaintext nhưng nonce khác → blob khác");
    }

    #[test]
    fn khoa_luu_va_nap_lai_on_dinh() {
        // load_or_create_key: sinh mới rồi nạp lại phải ra cùng khóa.
        let path = std::env::temp_dir().join(format!("mpm_key_{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let k1 = load_or_create_key(&path).unwrap();
        let k2 = load_or_create_key(&path).unwrap();
        assert_eq!(k1, k2, "nạp lại phải ra cùng khóa");

        // Trên Windows: file trên đĩa PHẢI là blob DPAPI, KHÔNG phải 32 byte trần.
        #[cfg(windows)]
        {
            let raw = std::fs::read(&path).unwrap();
            assert_ne!(
                raw.len(),
                32,
                "khóa không được để trần trên đĩa (phải DPAPI)"
            );
            assert_eq!(dpapi_unprotect(&raw).unwrap(), k1.to_vec());
        }
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_bao_ve_vong_tron() {
        let secret = b"khoa-bi-mat-32-byte-cua-snapshot";
        let wrapped = dpapi_protect(secret).expect("DPAPI protect");
        assert_ne!(wrapped.as_slice(), secret, "blob phải khác plaintext");
        assert_eq!(dpapi_unprotect(&wrapped).unwrap(), secret);
    }
}
