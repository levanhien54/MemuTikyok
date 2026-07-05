//! Sinh **fingerprint thiết bị** hợp lệ & duy nhất cho mỗi tài khoản TikTok.
//! Fingerprint được lưu vào CSDL (§ yêu cầu) và **áp lại mỗi lần khởi chạy** để
//! giữ nhất quán, chống TikTok liên kết chéo tài khoản (R-12).

use crate::error::{AppError, AppResult};
use crate::model::HardwareProfile;

/// Hồ sơ thiết bị THẬT, NHẤT QUÁN NỘI BỘ: model/brand/manufacturer/device khớp với
/// `ro.build.fingerprint`. Dùng thiết bị đời cũ (Android 8–11) — tránh Pixel mới/beta
/// bị gate hardware attestation (xem docs/ANTI_DETECTION_UPGRADE.md).
///
/// ⚠️ Mọi `fingerprint` phải là build THẬT của thiết bị (ByteDance có thể đối chiếu).
/// Mở rộng bảng bằng bộ đã kiểm chứng, KHÔNG bịa build id.
struct DeviceProfile {
    model: &'static str,
    brand: &'static str,
    manufacturer: &'static str,
    device: &'static str,
    fingerprint: &'static str,
    w: u32,
    h: u32,
    dpi: u32,
}

const DEVICES: &[DeviceProfile] = &[
    // Samsung Galaxy Note FE (bộ đã verify qua deep-research).
    DeviceProfile {
        model: "SM-N935F",
        brand: "samsung",
        manufacturer: "samsung",
        device: "gracerlte",
        fingerprint: "samsung/gracerltexx/gracerlte:8.0.0/R16NW/N935FXXS4BRK2:user/release-keys",
        w: 1440,
        h: 2560,
        dpi: 640,
    },
    // Samsung Galaxy S9 (SM-G960F, starlte).
    DeviceProfile {
        model: "SM-G960F",
        brand: "samsung",
        manufacturer: "samsung",
        device: "starlte",
        fingerprint: "samsung/starltexx/starlte:10/QP1A.190711.020/G960FXXUFFUJ1:user/release-keys",
        w: 1080,
        h: 2220,
        dpi: 480,
    },
    // Samsung Galaxy S8 (SM-G950F, dreamlte).
    DeviceProfile {
        model: "SM-G950F",
        brand: "samsung",
        manufacturer: "samsung",
        device: "dreamlte",
        fingerprint: "samsung/dreamltexx/dreamlte:9/PPR1.180610.011/G950FXXUCDUE1:user/release-keys",
        w: 1080,
        h: 2220,
        dpi: 480,
    },
    // Samsung Galaxy A50 (SM-A505F, a50).
    DeviceProfile {
        model: "SM-A505F",
        brand: "samsung",
        manufacturer: "samsung",
        device: "a50",
        fingerprint: "samsung/a50xx/a50:11/RP1A.200720.012/A505FDDU7CUB1:user/release-keys",
        w: 1080,
        h: 2340,
        dpi: 420,
    },
    // Xiaomi Redmi Note 8 (ginkgo).
    DeviceProfile {
        model: "Redmi Note 8",
        brand: "Redmi",
        manufacturer: "Xiaomi",
        device: "ginkgo",
        fingerprint: "Redmi/ginkgo/ginkgo:11/RP1A.200720.011/V12.5.1.0.RCOMIXM:user/release-keys",
        w: 1080,
        h: 2340,
        dpi: 440,
    },
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

/// Sinh một hồ sơ phần cứng ngẫu nhiên hợp lệ (bộ device NHẤT QUÁN + fingerprint thật).
pub fn generate() -> AppResult<HardwareProfile> {
    let pick = rand_bytes(1)?[0] as usize % DEVICES.len();
    let d = &DEVICES[pick];
    Ok(HardwareProfile {
        model: d.model.to_string(),
        brand: d.brand.to_string(),
        manufacturer: d.manufacturer.to_string(),
        imei: gen_imei()?,
        android_id: gen_android_id()?,
        mac: gen_mac()?,
        res_width: d.w,
        res_height: d.h,
        dpi: d.dpi,
        device: d.device.to_string(),
        build_fingerprint: d.fingerprint.to_string(),
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
