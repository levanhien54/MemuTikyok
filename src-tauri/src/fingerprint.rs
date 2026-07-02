//! Sinh **fingerprint thiết bị** hợp lệ & duy nhất cho mỗi tài khoản TikTok.
//! Fingerprint được lưu vào CSDL (§ yêu cầu) và **áp lại mỗi lần khởi chạy** để
//! giữ nhất quán, chống TikTok liên kết chéo tài khoản (R-12).

use crate::error::{AppError, AppResult};
use crate::model::HardwareProfile;

/// Mẫu thiết bị thật (model, brand, manufacturer, rộng, cao, dpi).
const DEVICES: &[(&str, &str, &str, u32, u32, u32)] = &[
    ("SM-G991B", "samsung", "samsung", 1080, 2400, 420), // Galaxy S21
    ("SM-G973F", "samsung", "samsung", 1080, 2280, 420), // Galaxy S10
    ("SM-A515F", "samsung", "samsung", 1080, 2400, 420), // Galaxy A51
    ("Pixel 6", "google", "Google", 1080, 2400, 420),
    ("Redmi Note 8", "Redmi", "Xiaomi", 1080, 2340, 440),
    ("M2101K6G", "Redmi", "Xiaomi", 1080, 2400, 440), // Redmi Note 10
    ("CPH2185", "OPPO", "OPPO", 720, 1600, 320),      // OPPO A54
    ("V2027", "vivo", "vivo", 1080, 2408, 408),       // vivo Y51
];

fn rand_bytes(n: usize) -> AppResult<Vec<u8>> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).map_err(|e| AppError::Io(format!("RNG lỗi: {e}")))?;
    Ok(buf)
}

/// Chữ số kiểm tra Luhn cho phần payload (dùng cho IMEI 15 chữ số).
fn luhn_check_digit(payload: &[u8]) -> u8 {
    let mut sum = 0u32;
    let mut double = true; // vị trí ngay trước check digit được nhân đôi
    for &d in payload.iter().rev() {
        let mut v = d as u32;
        if double {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
        double = !double;
    }
    ((10 - (sum % 10)) % 10) as u8
}

fn gen_imei() -> AppResult<String> {
    let digits: Vec<u8> = rand_bytes(14)?.iter().map(|b| b % 10).collect();
    let check = luhn_check_digit(&digits);
    let mut s: String = digits.iter().map(|d| (b'0' + d) as char).collect();
    s.push((b'0' + check) as char);
    Ok(s)
}

fn gen_android_id() -> AppResult<String> {
    Ok(rand_bytes(8)?.iter().map(|b| format!("{b:02x}")).collect())
}

/// MAC "locally administered" (byte đầu có bit 1 set, bit multicast clear).
fn gen_mac() -> AppResult<String> {
    let mut b = rand_bytes(6)?;
    b[0] = (b[0] & 0xfe) | 0x02;
    Ok(b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(":"))
}

/// Sinh một hồ sơ phần cứng ngẫu nhiên hợp lệ.
pub fn generate() -> AppResult<HardwareProfile> {
    let pick = rand_bytes(1)?[0] as usize % DEVICES.len();
    let (model, brand, manufacturer, w, h, dpi) = DEVICES[pick];
    Ok(HardwareProfile {
        model: model.to_string(),
        brand: brand.to_string(),
        manufacturer: manufacturer.to_string(),
        imei: gen_imei()?,
        android_id: gen_android_id()?,
        mac: gen_mac()?,
        res_width: w,
        res_height: h,
        dpi,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luhn_valid(imei: &str) -> bool {
        let digits: Vec<u32> = imei.chars().filter_map(|c| c.to_digit(10)).collect();
        let mut sum = 0u32;
        let mut double = false; // từ phải: check digit không nhân
        for &d in digits.iter().rev() {
            let mut v = d;
            if double {
                v *= 2;
                if v > 9 {
                    v -= 9;
                }
            }
            sum += v;
            double = !double;
        }
        sum % 10 == 0
    }

    #[test]
    fn imei_15_so_va_hop_le_luhn() {
        for _ in 0..50 {
            let hw = generate().unwrap();
            assert_eq!(hw.imei.len(), 15, "IMEI phải 15 chữ số");
            assert!(hw.imei.chars().all(|c| c.is_ascii_digit()));
            assert!(luhn_valid(&hw.imei), "IMEI {} phải hợp lệ Luhn", hw.imei);
        }
    }

    #[test]
    fn android_id_16_hex_mac_dinh_dang() {
        let hw = generate().unwrap();
        assert_eq!(hw.android_id.len(), 16);
        assert!(hw.android_id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hw.mac.split(':').count(), 6);
    }

    #[test]
    fn moi_lan_sinh_khac_nhau() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_ne!(a.imei, b.imei, "IMEI phải duy nhất");
        assert_ne!(a.android_id, b.android_id);
    }
}
