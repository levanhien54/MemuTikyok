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
///
/// ⚠️ ĐỘ PHÂN GIẢI phải MuMu-AN-TOÀN: rộng ≤ 1080, dpi ≤ 480. QHD (1440×2560@640)
/// LÀM CRASH MuMu Launcher2 ("keeps stopping") — đã kiểm chứng qua logcat. Máy QHD
/// thật (Note FE) chạy FHD 16:9 là bình thường, nên dùng 1080×1920 cho profile đó.
struct DeviceProfile {
    model: &'static str,
    brand: &'static str,
    manufacturer: &'static str,
    device: &'static str,
    fingerprint: &'static str,
    w: u32,
    h: u32,
    dpi: u32,
    tac: &'static str,
    soc_hardware: &'static str,
    board_platform: &'static str,
    gpu_egl: &'static str,
    security_patch: &'static str,
    build_characteristics: &'static str,
}

const DEVICES: &[DeviceProfile] = &[
    // Samsung Galaxy Note FE (bộ đã verify qua deep-research).
    DeviceProfile {
        model: "SM-N935F",
        brand: "samsung",
        manufacturer: "samsung",
        device: "gracerlte",
        fingerprint: "samsung/gracerltexx/gracerlte:8.0.0/R16NW/N935FXXS4BRK2:user/release-keys",
        // Note FE native QHD 1440×2560@640 làm CRASH MuMu Launcher2 → chạy FHD 16:9 (an toàn + thực tế).
        w: 1080,
        h: 1920,
        dpi: 480,
        tac: "35787508", // MoazEb tac-database: Galaxy Note FE SM-N935F
        soc_hardware: "samsungexynos8890", // từ init.samsungexynos8890.rc
        board_platform: "exynos5", // verbatim build.prop (Samsung legacy, không phải universal8890)
        gpu_egl: "mali",
        security_patch: "2018-11-01",
        build_characteristics: "",
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
        tac: "35798809", // Swappa IMEI SM-G960F
        soc_hardware: "samsungexynos9810",
        board_platform: "universal9810", // verbatim vendor build.prop
        gpu_egl: "mali",
        security_patch: "2021-10-01",
        build_characteristics: "",
    },
    // Samsung Galaxy S8 (SM-G950F, dreamlte).
    DeviceProfile {
        model: "SM-G950F",
        brand: "samsung",
        manufacturer: "samsung",
        device: "dreamlte",
        fingerprint:
            "samsung/dreamltexx/dreamlte:9/PPR1.180610.011/G950FXXUCDUE1:user/release-keys",
        w: 1080,
        h: 2220,
        dpi: 480,
        tac: "35903808",                   // Swappa IMEI SM-G950F
        soc_hardware: "samsungexynos8895", // build.hardware từ dump thật
        board_platform: "exynos5",         // verbatim (Samsung legacy)
        gpu_egl: "mali",
        security_patch: "2021-05-01",
        build_characteristics: "",
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
        tac: "35601010",                 // Swappa TAC SM-A505 family
        soc_hardware: "exynos9610",      // getprop thật (KHÔNG phải samsungexynos9610)
        board_platform: "universal9610", // verbatim vendor build.prop
        gpu_egl: "mali",
        security_patch: "2021-03-01",
        build_characteristics: "",
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
        tac: "86972204", // MoazEb tac-database: Redmi Note 8 M1908C3JG Global
        soc_hardware: "qcom",
        board_platform: "trinket", // SD665 platform, verbatim ginkgo dump
        gpu_egl: "adreno",
        security_patch: "2021-05-01",
        build_characteristics: "",
    },
    // ── Snapdragon (Adreno) — build.prop + TAC THẬT, fetch verbatim + verify chéo nguồn (2026-07-08).
    //    Đa dạng pool + device hiện đại. MỌI field coherence (soc/board/patch/tac) điền từ nguồn thật
    //    (Android-Dumps/tadiphone build.prop, MoazEb tac-database, Swappa IMEI) — KHÔNG bịa. Cổng
    //    `production_ready` vẫn lọc device thiếu field (chỉ SM-G981N thiếu TAC → imei rỗng, vẫn coherent).
    // Samsung Galaxy S20 5G Snapdragon (SM-G981N Korea, x1q, SD865/kona, Adreno 650).
    // src: gitlab Android-Dumps/samsung/x1q build.prop; cross: sfirmware.com/samsung-sm-g981n.
    DeviceProfile {
        model: "SM-G981N",
        brand: "samsung",
        manufacturer: "samsung",
        device: "x1q",
        fingerprint: "samsung/x1qksx/x1q:12/SP1A.210812.016/G981NKSU1GVE4:user/release-keys",
        w: 1080,
        h: 2400,
        dpi: 480,
        tac: "",
        soc_hardware: "qcom",
        board_platform: "kona",
        gpu_egl: "adreno",
        security_patch: "2022-05-01",
        build_characteristics: "",
    },
    // OnePlus 8T (KB2003 EEA, codename OnePlus8T/kebab, SD865/kona, Adreno 650).
    // src: dumps.tadiphone.dev/oneplus8t + LineageOS/AICP device_oneplus_kebab .mk (verbatim).
    DeviceProfile {
        model: "KB2003",
        brand: "OnePlus",
        manufacturer: "OnePlus",
        device: "OnePlus8T",
        fingerprint:
            "OnePlus/OnePlus8T_EEA/OnePlus8T:11/RP1A.201005.001/2011101425:user/release-keys",
        w: 1080,
        h: 2400,
        dpi: 480,
        tac: "86905705", // Swappa IMEI 869057055166607
        soc_hardware: "qcom",
        board_platform: "kona",
        gpu_egl: "adreno",
        security_patch: "2020-10-05", // XDA: OxygenOS 11.0.4.x giữ patch Oct-2020
        build_characteristics: "",
    },
    // Xiaomi POCO F2 Pro (lmi global, SD865/kona, Adreno 650).
    // src: dumps.tadiphone/xiaomi lmi build.prop (verbatim).
    DeviceProfile {
        model: "POCO F2 Pro",
        brand: "POCO",
        manufacturer: "Xiaomi",
        device: "lmi",
        fingerprint: "POCO/lmi_global/lmi:11/RKQ1.200826.002/V12.5.1.0.RJKMIXM:user/release-keys",
        w: 1080,
        h: 2400,
        dpi: 440,
        tac: "86693404", // MoazEb/tac-database: POCO F2 Pro M2004J11G Global
        soc_hardware: "qcom",
        board_platform: "kona",
        gpu_egl: "adreno",
        security_patch: "2021-06-01",
        build_characteristics: "",
    },
    // Google Pixel 5 (redfin, SD765G/lito, Adreno 620).
    // src: Android-Dumps google/redfin build.prop (verbatim).
    DeviceProfile {
        model: "Pixel 5",
        brand: "google",
        manufacturer: "Google",
        device: "redfin",
        fingerprint: "google/redfin/redfin:13/TQ3A.230705.001/10216780:user/release-keys",
        w: 1080,
        h: 2340,
        dpi: 440,
        tac: "35249411",        // 3 Swappa IMEI Pixel 5 khớp TAC
        soc_hardware: "redfin", // Pixel dùng codename cho ro.hardware (build.prop thật)
        board_platform: "lito",
        gpu_egl: "adreno",
        security_patch: "2023-07-05",
        build_characteristics: "",
    },
    // Samsung Galaxy S21 5G Snapdragon (SM-G991U US, o1q, SD888/lahaina, Adreno 660).
    // src: Android-Dumps samsung/o1q build.prop (verbatim).
    DeviceProfile {
        model: "SM-G991U",
        brand: "samsung",
        manufacturer: "samsung",
        device: "o1q",
        fingerprint: "samsung/o1qsqw/o1q:12/SP1A.210812.016/G991USQS4BUKK:user/release-keys",
        w: 1080,
        h: 2400,
        dpi: 480,
        tac: "35033626", // 2 Swappa IMEI SM-G991U khớp TAC
        soc_hardware: "qcom",
        board_platform: "lahaina",
        gpu_egl: "adreno",
        security_patch: "2021-12-01", // build BUKK = Dec-2021 patch (galaxyfirmware)
        build_characteristics: "",
    },
];

fn verified_tac(tac: &str) -> bool {
    tac.len() == 8 && tac.chars().all(|c| c.is_ascii_digit())
}

fn production_ready(d: &DeviceProfile) -> bool {
    !d.model.is_empty()
        && !d.brand.is_empty()
        && !d.manufacturer.is_empty()
        && !d.device.is_empty()
        && !d.fingerprint.is_empty()
        && d.w <= 1080
        && d.h <= 2400
        && (120..=480).contains(&d.dpi)
        && !d.soc_hardware.is_empty()
        && !d.board_platform.is_empty()
        && matches!(d.gpu_egl, "mali" | "adreno")
        && !d.security_patch.is_empty()
}

fn production_devices() -> Vec<&'static DeviceProfile> {
    DEVICES.iter().filter(|d| production_ready(d)).collect()
}

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

fn gen_imei(tac: &str) -> AppResult<String> {
    let tac = tac.trim();
    if tac.is_empty() {
        return Ok(String::new());
    }
    if !verified_tac(tac) {
        return Err(AppError::InvalidInput(format!(
            "TAC IMEI khong hop le cho device profile: {tac}"
        )));
    }
    let mut digits: Vec<u8> = tac.bytes().map(|b| b - b'0').collect();
    digits.extend(rand_bytes(6)?.iter().map(|b| b % 10));
    let check = luhn_check_digit(&digits);
    let mut s: String = digits.iter().map(|d| (b'0' + d) as char).collect();
    s.push((b'0' + check) as char);
    Ok(s)
}

fn pick_device_index(n: usize) -> AppResult<usize> {
    assert!(n > 0 && n <= 256);
    let limit = (256 / n) * n;
    loop {
        let b = rand_bytes(1)?[0] as usize;
        if b < limit {
            return Ok(b % n);
        }
    }
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
    let devices = production_devices();
    if devices.is_empty() {
        return Err(AppError::CommandFailed(
            "Khong co device profile production-ready".into(),
        ));
    }
    let pick = pick_device_index(devices.len())?;
    let d = devices[pick];
    Ok(HardwareProfile {
        model: d.model.to_string(),
        brand: d.brand.to_string(),
        manufacturer: d.manufacturer.to_string(),
        imei: gen_imei(d.tac)?,
        android_id: gen_android_id()?,
        mac: gen_mac()?,
        res_width: d.w,
        res_height: d.h,
        dpi: d.dpi,
        device: d.device.to_string(),
        build_fingerprint: d.fingerprint.to_string(),
        soc_hardware: d.soc_hardware.to_string(),
        board_platform: d.board_platform.to_string(),
        gpu_egl: d.gpu_egl.to_string(),
        security_patch: d.security_patch.to_string(),
        build_characteristics: d.build_characteristics.to_string(),
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
    fn imei_co_tac_thi_15_so_va_hop_le_luhn() {
        for _ in 0..50 {
            let imei = gen_imei("35847209").unwrap();
            assert_eq!(imei.len(), 15, "IMEI phai 15 chu so");
            assert!(imei.chars().all(|c| c.is_ascii_digit()));
            assert!(luhn_valid(&imei), "IMEI {imei} phai hop le Luhn");
        }
    }

    #[test]
    fn imei_dung_tac_khi_co() {
        let imei = gen_imei("35847209").unwrap();
        assert_eq!(imei.len(), 15);
        assert_eq!(&imei[..8], "35847209", "8 so dau phai la TAC");
        assert!(luhn_valid(&imei));
    }

    #[test]
    fn imei_rong_khi_chua_verify_tac() {
        let imei = gen_imei("").unwrap();
        assert_eq!(imei, "", "khong bia IMEI random khi TAC chua verify");
    }

    #[test]
    fn chon_device_khong_lech_modulo() {
        let devices = production_devices();
        let mut seen = vec![false; devices.len()];
        for _ in 0..500 {
            let i = pick_device_index(devices.len()).unwrap();
            assert!(i < devices.len());
            seen[i] = true;
        }
        assert!(
            seen.iter().all(|&b| b),
            "moi device production-ready deu phai duoc chon it nhat 1 lan"
        );
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
        assert_ne!(a.android_id, b.android_id);
        assert_ne!(a.mac, b.mac);
    }

    #[test]
    fn tat_ca_device_mumu_safe_va_egl_hop_le() {
        for d in DEVICES {
            assert!(d.w <= 1080, "{}: rong {} vuot MuMu-safe 1080", d.model, d.w);
            assert!(d.h <= 2400, "{}: cao {} vuot MuMu-safe 2400", d.model, d.h);
            assert!(
                (120..=480).contains(&d.dpi),
                "{}: dpi {} ngoai [120,480]",
                d.model,
                d.dpi
            );
            assert!(
                d.gpu_egl.is_empty() || d.gpu_egl == "mali" || d.gpu_egl == "adreno",
                "{}: gpu_egl '{}' khong hop le",
                d.model,
                d.gpu_egl
            );
            assert!(
                !d.model.is_empty() && !d.fingerprint.is_empty() && !d.device.is_empty(),
                "{}: thieu truong bat buoc",
                d.model
            );
        }
        assert!(
            DEVICES.len() >= 10,
            "catalog phai giu >=10 device sau khi them Snapdragon"
        );
        assert!(
            production_devices().len() >= 10,
            "pool production phai >=10 (moi device du coherence): {}",
            production_devices().len()
        );
    }

    #[test]
    fn production_pool_khong_de_prop_runtime_rong() {
        let devices = production_devices();
        assert!(
            devices.len() >= 2,
            "can toi thieu 2 device production-ready de random"
        );
        for d in devices {
            assert!(production_ready(d), "{}: khong production-ready", d.model);
            assert!(
                d.tac.is_empty() || verified_tac(d.tac),
                "{}: TAC phai rong hoac 8 chu so da verify",
                d.model
            );
        }
    }

    #[test]
    fn generate_chi_chon_device_production_ready() {
        for _ in 0..50 {
            let hw = generate().unwrap();
            assert!(
                !hw.soc_hardware.is_empty(),
                "{} thieu ro.hardware",
                hw.model
            );
            assert!(
                !hw.board_platform.is_empty(),
                "{} thieu ro.board.platform",
                hw.model
            );
            assert!(!hw.gpu_egl.is_empty(), "{} thieu ro.hardware.egl", hw.model);
            assert!(
                !hw.security_patch.is_empty(),
                "{} thieu security_patch",
                hw.model
            );
            if !hw.imei.is_empty() {
                assert_eq!(hw.imei.len(), 15);
                assert!(luhn_valid(&hw.imei));
            }
        }
    }
}
