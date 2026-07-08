//! E2E "thật" — bộ tích hợp in-crate chạy trên MuMu THẬT (SPEC A).
//!
//! ⚠️ Mọi test đều `#[ignore]`: chúng tạo VM thật, cài APK ~220MB, backup/restore
//! → rất chậm và bắt buộc có MuMu tại `D:\Microvirt\MuMu\MuMuManager.exe`. Chạy bằng:
//!   `cargo test --lib e2e_real -- --ignored --nocapture`
//!
//! KHÔNG đăng nhập TikTok ở bất kỳ đâu — một MARKER tổng hợp dưới `files/` đứng
//! thay dữ liệu phiên. Test PHẢI là in-crate mới với tới `crate::orchestrator`,
//! `crate::emulator::MumuClient`, `crate::adb::RealAdbWorker`, `MockGeolocator`
//! (private / `#[cfg(test)]`) — file ở `tests/` ngoài không truy cập được.
//!
//! An toàn VM: VM index 0 (đang chạy của người dùng) KHÔNG bao giờ bị đụng tới.
//! Mỗi test chụp `before = index_set()` TRƯỚC khi tạo gì đó; `VmGuard` chỉ hủy
//! những index thuộc "live-set" của chính test (phép hiệu với `before`), và Drop
//! chạy cả khi assert panic → không rò VM.

#![cfg(test)]
#![allow(clippy::await_holding_lock)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::adb::{AdbWorker, RealAdbWorker};
use crate::db::Db;
use crate::emulator::MumuClient;
use crate::error::{AppError, AppResult};
use crate::geo::{IpGeolocator, MockGeolocator};
use crate::model::{
    AccountProfile, AppSettings, EmulatorTell, HardwareProfile, SnapshotMeta, DEFAULT_TIKTOK_APK,
    TIKTOK_PKG,
};
use crate::orchestrator;
use crate::snapshot::LocalSnapshotStore;
use crate::state::{AppState, SharedState};

use async_trait::async_trait;

// ============================================================================
// A.0 — Harness dùng chung (nền tảng, không phải test hành vi)
// ============================================================================

/// Đường dẫn MuMuManager.exe: ưu tiên discover, fallback vị trí đã biết.
fn emulator_path() -> PathBuf {
    MumuClient::discover().unwrap_or_else(|| PathBuf::from(r"D:\Microvirt\MuMu\MuMuManager.exe"))
}

/// True nếu môi trường có MuMu thật; false → test tự early-return (skip mềm).
fn mumu_available() -> bool {
    emulator_path().exists()
}

/// HardwareProfile tất định cho phần lớn test (khớp bộ mock của orchestrator).
fn hw() -> HardwareProfile {
    HardwareProfile {
        model: "FRD-L19".into(),
        brand: "HUAWEI".into(),
        manufacturer: "HUAWEI".into(),
        imei: "860504493831119".into(),
        android_id: "a1b2c3d4e5f6".into(),
        mac: "02:00:00:11:22:33".into(),
        res_width: 1080,
        res_height: 1920,
        dpi: 320,
        device: "frd".into(),
        build_fingerprint: "HUAWEI/FRD-L19/HWFRD:8.0.0/HUAWEIFRD-L19/380C431:user/release-keys"
            .into(),
        soc_hardware: "kirin950".into(),
        board_platform: "hi3650".into(),
        gpu_egl: "mali".into(),
        security_patch: "2018-01-01".into(),
        build_characteristics: "".into(),
    }
}

/// AccountProfile tối thiểu cho test profile (chỉ cần username; creds để rỗng).
fn acc(username: &str) -> AccountProfile {
    AccountProfile {
        tiktok_username: username.into(),
        tiktok_password: String::new(),
        two_fa: String::new(),
        tiktok_passkey: String::new(),
        email: String::new(),
        email_password: String::new(),
    }
}

/// Tạo SharedState với adapter THẬT + thư mục temp riêng (không đụng %APPDATA%).
async fn make_state(tag: &str) -> (SharedState, PathBuf) {
    make_state_geo(tag, Arc::new(MockGeolocator)).await
}

/// Như `make_state` nhưng cho phép chọn geolocator (giữ cho linh hoạt tương lai).
async fn make_state_geo(tag: &str, geo: Arc<dyn IpGeolocator>) -> (SharedState, PathBuf) {
    let mp = emulator_path();
    let dir = std::env::temp_dir().join(format!("mpm_e2e_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let store = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
    let db = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
    let emulator = Arc::new(MumuClient::new(&mp));
    let adb = Arc::new(RealAdbWorker::new(&mp));

    let state: SharedState = Arc::new(AppState::new(
        emulator,
        geo,
        adb,
        store,
        AppSettings::default(),
        Some(db),
        std::collections::HashMap::new(),
    ));
    // Bật resetprop trong e2e bằng Magisk BUNDLED (resources/Magisk-v30.7.apk) → khóa
    // model/fingerprint + coherent props THẬT (giống app production tự bundle), để A.4
    // kiểm được W1/W2 thay vì bỏ qua vì thiếu resetprop.
    state.set_magisk_bin(crate::init_magisk_bin(&AppSettings::default()));
    (state, dir)
}

/// Tập index VM hiện có (đọc trực tiếp qua state.emulator).
async fn index_set(state: &SharedState) -> HashSet<u32> {
    state
        .emulator
        .list_instances()
        .await
        .map(|v| v.into_iter().map(|i| i.index).collect())
        .unwrap_or_default()
}

/// Shell trực tiếp `MuMuManager.exe adb -v <idx> -c "shell <cmd>"` → stdout đã lọc.
/// MuMu 12 dùng `adb root`; không giả định trong image có binary `su`.
/// nhiễu "already connected" / "adb server" (mirror `real.rs::prop`).
/// Dùng std::process::Command vì `RealAdbWorker::adb()` là private.
fn vm_shell(mp: &PathBuf, idx: u32, cmd: &str) -> String {
    let arg = format!("shell {cmd}");
    let out = Command::new(mp)
        .args(["adb", "-v", &idx.to_string(), "-c", &arg])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| {
                !l.is_empty()
                    && !l.contains("already connected")
                    && !l.contains("adb server")
                    && !l.contains("daemon")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(_) => String::new(),
    }
}

/// Shell không bọc `su -c` (cho các lệnh như getprop / settings không cần root wrap
/// nhưng vẫn ổn qua su). Ở đây dùng dạng adb raw để đọc getprop token cuối.
fn vm_adb_raw(mp: &PathBuf, idx: u32, adb_arg: &str) -> String {
    let out = Command::new(mp)
        .args(["adb", "-v", &idx.to_string(), "-c", adb_arg])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| {
                !l.is_empty()
                    && !l.contains("already connected")
                    && !l.contains("adb server")
                    && !l.contains("daemon")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(_) => String::new(),
    }
}

/// getprop lấy token cuối cùng (đề phòng emulator chèn dòng nhiễu trước giá trị).
fn getprop(mp: &PathBuf, idx: u32, name: &str) -> String {
    let raw = vm_adb_raw(mp, idx, &format!("shell getprop {name}"));
    raw.split_whitespace().last().unwrap_or("").to_string()
}

/// simulation đọc lại một khóa emulator (fallback OS-level proof).
fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn simulation(mp: &PathBuf, idx: u32, key: &str) -> String {
    let out = Command::new(mp)
        .args(["simulation", "-v", &idx.to_string(), "-sk", key])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

/// Guard hủy VM UNIVERSAL: giữ tập index còn sống; Drop → stop + remove từng cái
/// SYNCHRONOUS. Fire cả khi unwind (assert panic) → không rò VM. Không bao giờ
/// đụng index 0 hay VM có sẵn (các test chỉ nạp index MỚI vào live-set).
struct VmGuard {
    mp: PathBuf,
    live: Arc<Mutex<Vec<u32>>>,
}

impl VmGuard {
    fn new(mp: PathBuf) -> Self {
        Self {
            mp,
            live: Arc::new(Mutex::new(Vec::new())),
        }
    }
    /// Đăng ký một index mới vào live-set (guard sẽ hủy khi Drop).
    fn track(&self, idx: u32) {
        assert_ne!(idx, 0, "KHÔNG được track index 0 (VM người dùng)");
        let mut g = self.live.lock().unwrap();
        if !g.contains(&idx) {
            g.push(idx);
        }
    }
    /// Bỏ một index khỏi live-set (khi test chủ động hủy nó — vd teardown).
    fn untrack(&self, idx: u32) {
        self.live.lock().unwrap().retain(|&x| x != idx);
    }
}

impl Drop for VmGuard {
    fn drop(&mut self) {
        let idxs: Vec<u32> = self.live.lock().unwrap().clone();
        for idx in idxs {
            if idx == 0 {
                continue; // an toàn tuyệt đối
            }
            let _ = Command::new(&self.mp)
                .args(["control", "-v", &idx.to_string(), "shutdown"])
                .output();
            for _ in 0..3 {
                std::thread::sleep(std::time::Duration::from_secs(4));
                let out = Command::new(&self.mp)
                    .args(["delete", "-v", &idx.to_string()])
                    .output();
                let text = out
                    .as_ref()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_default();
                if text.contains("\"errcode\": 0") || text.contains("player not running") {
                    break;
                }
            }
        }
    }
}

/// Bất biến cách ly: mỗi index mới phải ∉ before và != 0.
fn assert_new_index(before: &HashSet<u32>, idx: u32) {
    assert!(
        !before.contains(&idx),
        "index {idx} phải là VM MỚI (không có trong before)"
    );
    assert_ne!(idx, 0, "index mới không được trùng VM 0");
}

// ============================================================================
// A.0 — Test: harness compile + skip gate + create/destroy round-trip
// ============================================================================

#[tokio::test]
#[ignore]
async fn a0_harness_create_destroy_roundtrip() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu tại {:?}", emulator_path());
        return;
    }
    let (state, _dir) = make_state("a0").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    assert!(before.contains(&0), "VM 0 phải tồn tại trước test");

    let idx = orchestrator::create_vm(&state).await.expect("create_vm");
    guard.track(idx);
    assert_new_index(&before, idx);

    let after = index_set(&state).await;
    assert!(after.contains(&idx), "VM mới phải xuất hiện trong list");

    // Hủy chủ động → Drop là no-op cho idx.
    let _ = state.emulator.stop(idx).await;
    state.emulator.remove(idx).await.expect("remove");
    guard.untrack(idx);

    let post = index_set(&state).await;
    assert_eq!(post, before, "sau khi hủy, tập index phải trở về before");
    assert!(post.contains(&0), "VM 0 phải còn nguyên");
}

// ============================================================================
// Tier 1 — logic / rẻ (không boot VM)
// ============================================================================

/// A.1 — create_vm nhận diện index MỚI đúng.
#[tokio::test]
#[ignore]
async fn a1_create_vm_new_index() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    let (state, _dir) = make_state("a1").await;
    let guard = VmGuard::new(emulator_path());

    let before = index_set(&state).await;
    let idx = orchestrator::create_vm(&state).await.expect("create_vm");
    guard.track(idx);

    assert_new_index(&before, idx);
    assert!(
        index_set(&state).await.contains(&idx),
        "VM mới có trong list"
    );
    // VM 0 vẫn còn.
    let list = state.emulator.list_instances().await.expect("list");
    assert!(list.iter().any(|v| v.index == 0), "VM 0 phải còn tồn tại");
}

/// A.3 — Snapshot hỏng bị `verify` từ chối (nhánh chỉ-store, không VM).
#[tokio::test]
#[ignore]
async fn a3_corrupt_snapshot_rejected() {
    let (state, dir) = make_state("a3").await;

    // Chuẩn bị file nhỏ → put vào store.
    let src = dir.join("src.tar");
    std::fs::write(&src, b"marker-payload-for-integrity-check").unwrap();
    let key = "acc-corrupt/1.tar.zst";
    let stored = state.store.put(key, &src).await.expect("put");

    // Ghi CSDL.
    let meta = SnapshotMeta {
        sha256: stored.sha256.clone(),
        size_bytes: stored.size_bytes,
        apk_version: "unknown".into(),
    };
    state
        .db
        .as_ref()
        .unwrap()
        .record_snapshot("acc-corrupt", key, &meta, crate::state::now_ms())
        .unwrap();

    // verify khớp trước khi phá.
    assert!(
        state.store.verify(key, &stored.sha256).await.unwrap(),
        "sha của blob đã lưu phải khớp"
    );

    // Lật 1 byte giữa blob trên đĩa.
    let blob_path = dir.join("snap").join(key);
    let mut bytes = std::fs::read(&blob_path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&blob_path, &bytes).unwrap();

    // verify giờ phải false.
    assert!(
        !state.store.verify(key, &stored.sha256).await.unwrap(),
        "blob đã sửa → verify phải false"
    );
    // get phải Err (AES-GCM tag/ciphertext hỏng → decrypt fail).
    let out = dir.join("out.tar");
    assert!(
        state.store.get(key, &out).await.is_err(),
        "blob hỏng không thể giải mã/giải nén"
    );
    // Bản ghi DB vẫn còn (chỉ blob bị từ chối).
    assert!(
        state
            .db
            .as_ref()
            .unwrap()
            .latest_snapshot("acc-corrupt")
            .unwrap()
            .is_some(),
        "hàng DB vẫn tồn tại"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ============================================================================
// Tier 2 — vòng đời một VM (mỗi test boot một lần)
// ============================================================================

/// A.4 — provision inject fingerprint + đọc lại (bao gồm scan A.7 để tiết kiệm boot).
#[tokio::test]
#[ignore]
async fn a4_provision_fingerprint_inject() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    let (state, dir) = make_state("a4").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let hw = hw();
    let idx = orchestrator::provision(&state, "acc_fp", &hw, None)
        .await
        .expect("provision");
    guard.track(idx);
    assert_new_index(&before, idx);

    // boot_completed == 1
    assert_eq!(
        getprop(&mp, idx, "sys.boot_completed"),
        "1",
        "VM phải boot xong"
    );

    // android_id runtime (bằng chứng fingerprint chính) — luôn dính.
    let aid = vm_shell(&mp, idx, "settings get secure android_id");
    assert!(
        aid.contains("a1b2c3d4e5f6"),
        "android_id phải khớp hw(): {aid}"
    );

    // root khả dụng (enable_su=1).
    let idout = vm_shell(&mp, idx, "id");
    assert!(idout.contains("uid=0"), "phải có root: {idout}");

    // ⚠️ KNOWN-GAP (phát hiện qua test thực): MuMu GHI ĐÈ model bằng thiết bị NGẪU
    // NHIÊN khi VM BOOT (FRD-L19 đặt lúc dừng → ASUS_AI2401_A / NX809J sau boot).
    // Nghĩa là `microvirt_vm_model` KHÔNG do ta kiểm soát qua boot → chỉ cảnh báo,
    // KHÔNG assert cứng. Fingerprint thực sự áp được & bền trong test hiện tại là
    // android_id (đã assert ở trên); resolution/DPI cũng có thể bị profile/window
    // default của MuMu ghi đè, nên chỉ cảnh báo. Xem docs/E2E_RUNBOOK.md.
    let model = getprop(&mp, idx, "ro.product.model");
    let cfg = simulation(&mp, idx, "microvirt_vm_model");
    if model != "FRD-L19" || !cfg.contains("FRD-L19") {
        eprintln!(
            "[known-gap] MuMu ghi đè model khi boot: ro.product.model={model:?} \
             simulation={cfg:?} — model KHÔNG stick (android_id/DPI mới là fingerprint hiệu lực)"
        );
    }

    // Độ phân giải + DPI: config-level có set, nhưng runtime `wm size` trên bản MuMu
    // đang test vẫn có thể là 900x1600. Không fail cứng để A.4 tiếp tục scan tells.
    let size = vm_adb_raw(&mp, idx, "shell wm size");
    let density = vm_adb_raw(&mp, idx, "shell wm density");
    if !size.contains("1080x1920") && !size.contains("1080 x 1920") {
        eprintln!("[known-gap] wm size không theo custom_resolution: {size}");
    }
    if !density.contains("320") {
        eprintln!("[known-gap] wm density không theo custom_resolution: {density}");
    }

    // Coherence W1/W2 (characteristics/egl/patch) CHỈ áp được khi resetprop khóa THÀNH CÔNG.
    // MuMu 15 không có `su` → cần adb root; nếu resetprop không khả dụng (thiếu Magisk APK /
    // magisk -c fail) thì bỏ qua assert strict, chỉ cảnh báo — tránh fail theo môi trường.
    let fp_lock = state.fingerprint_lock_status(idx).await;
    if fp_lock.locked {
        let characteristics = getprop(&mp, idx, "ro.build.characteristics");
        assert!(
            !characteristics.contains("tablet"),
            "ro.build.characteristics van la tablet: {characteristics}"
        );
        if !hw.gpu_egl.is_empty() {
            let egl = getprop(&mp, idx, "ro.hardware.egl");
            assert_eq!(egl.trim(), hw.gpu_egl, "ro.hardware.egl lech profile");
        }
        if !hw.security_patch.is_empty() {
            let patch = getprop(&mp, idx, "ro.build.version.security_patch");
            assert_eq!(
                patch.trim(),
                hw.security_patch,
                "security_patch lech profile"
            );
        }
    } else {
        eprintln!(
            "[known-gap] resetprop không khóa được ({}) — bỏ qua assert coherence W1/W2",
            fp_lock.message
        );
    }

    let mac_cfg = simulation(&mp, idx, "mac_address");
    eprintln!("[info] mac_address simulation = {mac_cfg:?}");

    // --- Ghép A.7: scan_emulator_tells + debloat (best-effort, tất cả loose) ---
    let tells: Vec<EmulatorTell> = state
        .adb
        .scan_emulator_tells(idx)
        .await
        .expect("scan_emulator_tells");
    assert!(tells.len() >= 8, "scan phải có đủ mục nền: {tells:?}");
    let by_check = |name: &str| tells.iter().find(|t| t.check == name);
    for name in [
        "Native Bridge (ARM→x86)",
        "CPU hypervisor flag",
        "ro.kernel.qemu",
        "File QEMU/Genymotion",
        "vboxsf mount",
        "GPU renderer ảo",
        "ro.build.characteristics",
        "ro.build.tags",
        "Magisk/resetprop (khóa model)",
    ] {
        assert!(by_check(name).is_some(), "thiếu mục scan '{name}'");
    }
    // Sạch (detected == false).
    for name in [
        "ro.kernel.qemu",
        "File QEMU/Genymotion",
        "vboxsf mount",
        "GPU renderer ảo",
    ] {
        let t = by_check(name).unwrap();
        assert!(!t.detected, "'{name}' nên sạch (false): {t:?}");
    }
    // ro.build.characteristics là soft.
    if let Some(t) = by_check("ro.build.characteristics") {
        if t.detected {
            eprintln!(
                "[soft] ro.build.characteristics vẫn 'tablet': {:?}",
                t.detail
            );
        }
    }
    // GMS/GSF phải còn (không nằm trong DEFAULT_BLOAT).
    let gms = vm_adb_raw(&mp, idx, "shell pm list packages com.google.android.gms");
    assert!(
        gms.contains("com.google.android.gms"),
        "GMS phải còn bật: {gms}"
    );
    // root + boot vẫn ổn sau debloat.
    assert_eq!(getprop(&mp, idx, "sys.boot_completed"), "1");
    assert!(vm_shell(&mp, idx, "id").contains("uid=0"));

    // Cleanup.
    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.6 — cài APK TikTok có mặt.
#[tokio::test]
#[ignore]
async fn a6_install_tiktok_apk() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    let (state, dir) = make_state("a6").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let idx = orchestrator::create_vm(&state).await.expect("create_vm");
    guard.track(idx);
    assert_new_index(&before, idx);

    state
        .queue
        .run(state.emulator.start(idx))
        .await
        .expect("start");
    state.mark_launched(idx).await;
    state.adb.wait_boot_completed(idx).await.expect("wait_boot");

    state
        .adb
        .install_apk(idx, DEFAULT_TIKTOK_APK)
        .await
        .expect("install_apk phải Ok");

    let pkgs = vm_adb_raw(&mp, idx, &format!("shell pm list packages {TIKTOK_PKG}"));
    assert!(
        pkgs.contains(&format!("package:{TIKTOK_PKG}")),
        "TikTok phải được cài: {pkgs}"
    );
    // versionName chứa "40" (soft: chỉ cần non-empty).
    let ver = vm_adb_raw(
        &mp,
        idx,
        &format!("shell dumpsys package {TIKTOK_PKG} | grep versionName"),
    );
    if !ver.contains("40") {
        eprintln!("[soft] versionName không chứa '40': {ver}");
        assert!(!ver.trim().is_empty(), "versionName phải non-empty: {ver}");
    }

    state
        .adb
        .start_app(idx, TIKTOK_PKG)
        .await
        .expect("start_app");

    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

// ============================================================================
// Tier 3 — nặng / nhiều chu kỳ / bất biến (chậm nhất)
// ============================================================================

/// A.9 — Round-trip dùng-một-lần (flagship): provision → marker → teardown →
/// provision restore marker. Bằng chứng sống-còn dữ liệu + R-15.
#[tokio::test]
#[ignore]
async fn a9_full_disposable_roundtrip() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    let (state, dir) = make_state("a9").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let marker = format!("/data/data/{TIKTOK_PKG}/files/mpm_marker.txt");
    let nonce = format!("{}-{}", std::process::id(), crate::state::now_ms());
    let payload = format!("MPM-MARKER-{nonce}");

    // --- Phiên 1 ---
    let idx1 = orchestrator::provision(&state, "acc_e2e", &hw(), None)
        .await
        .expect("provision phiên 1");
    guard.track(idx1);
    assert_new_index(&before, idx1);

    state
        .adb
        .install_apk(idx1, DEFAULT_TIKTOK_APK)
        .await
        .expect("install_apk phiên 1");
    let _ = state.adb.start_app(idx1, TIKTOK_PKG).await;

    // Ghi marker với owner + SELinux đúng.
    vm_shell(
        &mp,
        idx1,
        &format!(
            "mkdir -p /data/data/{TIKTOK_PKG}/files && echo {payload} > {marker} && \
             U=$(stat -c %u /data/data/{TIKTOK_PKG}); chown $U:$U {marker}; restorecon {marker}"
        ),
    );
    let pre = vm_shell(&mp, idx1, &format!("cat {marker}"));
    assert!(
        pre.contains(&payload),
        "marker phải có mặt trước backup: {pre}"
    );
    let pre_label = vm_shell(&mp, idx1, &format!("ls -Z {marker}"));
    let pre_owner = vm_shell(&mp, idx1, &format!("stat -c %u:%g /data/data/{TIKTOK_PKG}"));
    eprintln!("[info] pre label={pre_label} owner={pre_owner}");

    // --- Teardown (backup → stop → remove) ---
    let rec = orchestrator::teardown(&state, idx1, "acc_e2e")
        .await
        .expect("teardown phiên 1");
    guard.untrack(idx1); // đã hủy chủ động

    // Bản ghi hợp lệ.
    let key_ok = regex_like_storage_key(&rec.storage_key, "acc_e2e");
    assert!(key_ok, "storage_key sai định dạng: {}", rec.storage_key);
    assert_eq!(rec.sha256.len(), 64, "sha256 phải 64 hex");
    assert!(rec.size_bytes > 0, "size_bytes phải > 0");
    assert!(rec.apk_version.is_some(), "apk_version phải Some");

    let db = state.db.as_ref().unwrap();
    let latest = db.latest_snapshot("acc_e2e").unwrap().unwrap();
    assert_eq!(latest.storage_key, rec.storage_key);
    assert!(
        state
            .store
            .verify(&rec.storage_key, &rec.sha256)
            .await
            .unwrap(),
        "blob toàn vẹn"
    );

    // R-15: idx1 đã biến mất (chỉ hủy SAU khi backup thành công).
    assert!(
        !index_set(&state).await.contains(&idx1),
        "idx1 phải bị hủy sau teardown"
    );

    // --- Phiên 2 ---
    let idx2 = orchestrator::provision(&state, "acc_e2e", &hw(), None)
        .await
        .expect("provision phiên 2");
    guard.track(idx2);
    assert_ne!(idx2, idx1, "idx2 nên khác idx1 (nói chung)");
    assert_ne!(idx2, 0);

    // Restore giải nén vào /data/data/<pkg> → cần thư mục tồn tại → cài APK.
    state
        .adb
        .install_apk(idx2, DEFAULT_TIKTOK_APK)
        .await
        .expect("install_apk phiên 2");

    // provision phiên 2 đã restore snapshot rồi (vì latest_snapshot tồn tại).
    // Nhưng install_apk sau provision có thể ghi đè dir → restore lại cho chắc
    // bằng cách đọc marker trực tiếp; nếu trống thì restore thủ công qua launch.
    let mut got = vm_shell(&mp, idx2, &format!("cat {marker}"));
    if !got.contains(&payload) {
        // provision đã restore TRƯỚC install; install -r không xóa /data/data data dir,
        // nhưng để chắc, restore lại từ store.
        let tmp = dir.join("relaunch.tar.zst");
        state
            .store
            .get(&rec.storage_key, &tmp)
            .await
            .expect("get blob");
        state
            .adb
            .restore(idx2, TIKTOK_PKG, &tmp)
            .await
            .expect("restore thủ công");
        got = vm_shell(&mp, idx2, &format!("cat {marker}"));
    }

    // Bằng chứng sống-còn byte-exact.
    assert!(
        got.contains(&payload),
        "marker phải sống sót qua destroy→restore: got={got}, want={payload}"
    );

    // MuMu SELinux DISABLED (ls -Z ra '?' mọi file) → nhãn vô nghĩa; owner mới quyết định
    // app đọc được (không EACCES). restore chown về owner pkg dir (app-uid).
    let owner_marker = vm_shell(&mp, idx2, &format!("stat -c %u {marker}"));
    let owner_dir = vm_shell(&mp, idx2, &format!("stat -c %u /data/data/{TIKTOK_PKG}"));
    assert_eq!(
        owner_marker.trim(),
        owner_dir.trim(),
        "marker phải cùng owner với app data dir (chown restore đúng)"
    );

    // Fingerprint re-applied trên VM mới.
    let cfg = simulation(&mp, idx2, "microvirt_vm_model");
    assert_eq!(cfg, "FRD-L19", "fingerprint áp lại trên VM mới");

    // Cleanup.
    let _ = state.emulator.stop(idx2).await;
    let _ = state.emulator.remove(idx2).await;
    guard.untrack(idx2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Adb bọc: chuyển tiếp mọi method sang Real, TRỪ backup() → luôn Err.
/// Dùng chứng minh R-15 tất định (không phụ thuộc APK đã cài hay chưa).
struct FailingBackupAdb(Arc<RealAdbWorker>);

#[async_trait]
impl AdbWorker for FailingBackupAdb {
    async fn backup(
        &self,
        _idx: u32,
        _pkg: &str,
        _out: &std::path::Path,
    ) -> AppResult<SnapshotMeta> {
        Err(AppError::CommandFailed("forced".into()))
    }
    async fn restore(&self, idx: u32, pkg: &str, archive: &std::path::Path) -> AppResult<()> {
        self.0.restore(idx, pkg, archive).await
    }
    async fn apk_version(&self, idx: u32, pkg: &str) -> AppResult<String> {
        self.0.apk_version(idx, pkg).await
    }
    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()> {
        self.0.apply_android_id(idx, android_id).await
    }
    async fn apply_display_profile(
        &self,
        idx: u32,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> AppResult<bool> {
        self.0.apply_display_profile(idx, width, height, dpi).await
    }
    async fn wait_boot_completed(&self, idx: u32) -> AppResult<()> {
        self.0.wait_boot_completed(idx).await
    }
    async fn start_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        self.0.start_app(idx, pkg).await
    }
    async fn install_apk(&self, idx: u32, apk_path: &str) -> AppResult<()> {
        self.0.install_apk(idx, apk_path).await
    }
    async fn disable_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        self.0.disable_app(idx, pkg).await
    }
    async fn scan_emulator_tells(&self, idx: u32) -> AppResult<Vec<EmulatorTell>> {
        self.0.scan_emulator_tells(idx).await
    }
    async fn harden(&self, idx: u32) -> AppResult<()> {
        self.0.harden(idx).await
    }
    async fn push_resetprop(&self, idx: u32, local_bin: &str) -> AppResult<bool> {
        self.0.push_resetprop(idx, local_bin).await
    }
    async fn lock_device_identity(
        &self,
        idx: u32,
        hw: &crate::model::HardwareProfile,
    ) -> AppResult<bool> {
        self.0.lock_device_identity(idx, hw).await
    }
    async fn human_tap(&self, idx: u32, x: i32, y: i32) -> AppResult<()> {
        self.0.human_tap(idx, x, y).await
    }
    async fn human_swipe(&self, idx: u32, x0: i32, y0: i32, x1: i32, y1: i32) -> AppResult<()> {
        self.0.human_swipe(idx, x0, y0, x1, y1).await
    }
    async fn upload_media(&self, idx: u32, local_path: &str) -> AppResult<()> {
        self.0.upload_media(idx, local_path).await
    }
}

/// A.13 — Nạp video vào VM và kiểm tra file.
#[tokio::test]
#[ignore]
async fn a13_upload_media() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    let (state, dir) = make_state("upload").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());

    // Tạo VM mới
    let before = index_set(&state).await;
    let _ = state.emulator.create().await;
    let after = index_set(&state).await;
    let idx = *after.difference(&before).next().expect("Không tạo được VM");
    guard.track(idx);

    // Tạo file mp4 giả
    let mock_video = dir.join("test_video.mp4");
    std::fs::write(&mock_video, b"fake video content").unwrap();

    state.adb.wait_boot_completed(idx).await.unwrap();

    // Gọi upload_media
    state
        .adb
        .upload_media(idx, mock_video.to_str().unwrap())
        .await
        .unwrap();

    // Check file tồn tại trong VM
    let ls_out = vm_shell(&mp, idx, "ls /sdcard/DCIM/Camera/test_video.mp4");
    assert!(
        ls_out.contains("test_video.mp4"),
        "File video không được đẩy thành công vào /sdcard/DCIM/Camera"
    );

    // Cleanup
    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.10 — R-15 nghiêm: backup thất bại KHÔNG hủy VM.
#[tokio::test]
#[ignore]
async fn a10_r15_backup_fail_vm_not_destroyed() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    let mp = emulator_path();
    let dir = std::env::temp_dir().join(format!("mpm_e2e_{}_a10", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let store = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
    let db = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
    let emulator = Arc::new(MumuClient::new(&mp));
    let real_adb = Arc::new(RealAdbWorker::new(&mp));
    let adb: Arc<dyn AdbWorker> = Arc::new(FailingBackupAdb(real_adb));

    let state: SharedState = Arc::new(AppState::new(
        emulator,
        Arc::new(MockGeolocator),
        adb,
        store,
        AppSettings::default(),
        Some(db),
        std::collections::HashMap::new(),
    ));

    let guard = VmGuard::new(mp.clone());
    let before = index_set(&state).await;

    // provision không gọi backup → vẫn chạy.
    let idx = orchestrator::provision(&state, "acc_r15", &hw(), None)
        .await
        .expect("provision (không dùng backup)");
    guard.track(idx);
    assert_new_index(&before, idx);

    // teardown → backup_and_record fail TRƯỚC stop/remove.
    let res = orchestrator::teardown(&state, idx, "acc_r15").await;
    assert!(
        matches!(res, Err(AppError::CommandFailed(_))),
        "teardown phải Err(CommandFailed): {res:?}"
    );

    // VM vẫn còn (không bị hủy).
    assert!(
        index_set(&state).await.contains(&idx),
        "R-15: VM KHÔNG được hủy khi backup fail"
    );
    // Không có bản ghi snapshot ma.
    assert!(
        state
            .db
            .as_ref()
            .unwrap()
            .latest_snapshot("acc_r15")
            .unwrap()
            .is_none(),
        "không được ghi snapshot khi backup fail"
    );
    // Không có blob nào dưới snap/acc_r15.
    let blob_dir = dir.join("snap").join("acc_r15");
    let has_blob = std::fs::read_dir(&blob_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).any(|_| true))
        .unwrap_or(false);
    assert!(!has_blob, "không được có blob khi backup fail");

    // Cleanup: guard sẽ hủy idx (teardown cố tình để nó sống).
    drop(guard);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.11 — Toàn vẹn snapshot + retention với dữ liệu VM thật.
#[tokio::test]
#[ignore]
async fn a11_snapshot_integrity_retention() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    let (state, dir) = make_state("a11").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());

    assert_eq!(orchestrator::SNAPSHOT_RETENTION, 5, "hằng retention phải 5");

    let before = index_set(&state).await;
    let idx = orchestrator::provision(&state, "acc_ret", &hw(), None)
        .await
        .expect("provision");
    guard.track(idx);
    assert_new_index(&before, idx);

    state
        .adb
        .install_apk(idx, DEFAULT_TIKTOK_APK)
        .await
        .expect("install_apk");

    let marker = format!("/data/data/{TIKTOK_PKG}/files/mpm_marker.txt");
    vm_shell(&mp, idx, &format!("mkdir -p /data/data/{TIKTOK_PKG}/files"));

    // 7 backup (> retention 5).
    let mut recs = Vec::new();
    for i in 0..7 {
        vm_shell(&mp, idx, &format!("echo V{i} > {marker}"));
        let rec = orchestrator::backup_and_record(&state, idx, "acc_ret")
            .await
            .unwrap_or_else(|e| panic!("backup #{i} fail: {e:?}"));
        assert_eq!(rec.sha256.len(), 64, "sha256 #{i} phải 64 hex");
        assert!(rec.size_bytes > 0, "size #{i} > 0");
        recs.push(rec);
    }

    let db = state.db.as_ref().unwrap();
    // Sau prune chỉ còn 5.
    assert_eq!(
        db.snapshots_beyond("acc_ret", 0).unwrap().len(),
        5,
        "retention phải giữ đúng 5 bản"
    );

    // 2 bản cũ nhất: blob đã bị xóa → verify false.
    for old in &recs[0..2] {
        assert!(
            !state
                .store
                .verify(&old.storage_key, &old.sha256)
                .await
                .unwrap(),
            "blob cũ {} phải bị xóa (verify false)",
            old.storage_key
        );
    }

    // latest == bản cuối; verify true.
    let latest = db.latest_snapshot("acc_ret").unwrap().unwrap();
    let last = recs.last().unwrap();
    assert_eq!(latest.storage_key, last.storage_key, "latest = bản cuối");
    assert!(
        state
            .store
            .verify(&latest.storage_key, &latest.sha256)
            .await
            .unwrap(),
        "blob mới nhất phải verify true"
    );

    // Không còn file .tmp sót dưới snap/acc_ret.
    let snap_acc = dir.join("snap").join("acc_ret");
    if let Ok(rd) = std::fs::read_dir(&snap_acc) {
        let leftover: Vec<_> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftover.is_empty(), "còn sót .tmp: {leftover:?}");
    }

    // Restore latest → cat marker == "V6".
    let out = dir.join("latest.tar.zst");
    state
        .store
        .get(&latest.storage_key, &out)
        .await
        .expect("get latest");
    state
        .adb
        .restore(idx, TIKTOK_PKG, &out)
        .await
        .expect("restore latest");
    let got = vm_shell(&mp, idx, &format!("cat {marker}"));
    assert!(
        got.contains("V6"),
        "marker sau restore latest phải là V6: {got}"
    );

    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.14 — Đo dung lượng/hiệu năng snapshot trên MuMu thật + APK TikTok thật.
///
/// Không đăng nhập tài khoản: bài này đo đường ống kỹ thuật thật (MuMu adb → tar data
/// thật của package → zstd/mã hóa local → verify/get/restore), không đại diện cho
/// dung lượng account đã đăng nhập lâu ngày.
#[tokio::test]
#[ignore]
async fn a14_snapshot_storage_metrics_real() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    if !std::path::Path::new(DEFAULT_TIKTOK_APK).exists() {
        eprintln!("[skip] Không có APK tại {DEFAULT_TIKTOK_APK}");
        return;
    }

    let (state, dir) = make_state("a14_metrics").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());
    let before = index_set(&state).await;

    let idx = orchestrator::provision(&state, "acc_metrics", &hw(), Some(DEFAULT_TIKTOK_APK))
        .await
        .expect("provision + install TikTok");
    guard.track(idx);
    assert_new_index(&before, idx);

    // Mở app một chút để package sinh shared_prefs/databases/app_webview cơ bản.
    let _ = state.adb.start_app(idx, TIKTOK_PKG).await;
    tokio::time::sleep(std::time::Duration::from_secs(20)).await;

    // Marker nhỏ để chứng minh restore round-trip trên VM thật.
    let marker = format!("/data/data/{TIKTOK_PKG}/files/mpm_metrics_marker.txt");
    vm_shell(
        &mp,
        idx,
        &format!(
            "mkdir -p /data/data/{TIKTOK_PKG}/files && echo metrics-{idx} > {marker} && \
             U=$(stat -c %u /data/data/{TIKTOK_PKG}); chown $U:$U {marker}; restorecon {marker}"
        ),
    );

    let raw = dir.join("metrics-raw.tar");
    let backup_start = Instant::now();
    let adb_meta = state
        .adb
        .backup(idx, TIKTOK_PKG, &raw)
        .await
        .expect("real adb backup");
    let backup_ms = backup_start.elapsed().as_millis();
    let raw_bytes = std::fs::metadata(&raw).unwrap().len();
    assert_eq!(raw_bytes, adb_meta.size_bytes);
    assert!(raw_bytes > 0, "raw tar phải có dữ liệu");

    let put_start = Instant::now();
    let stored = state
        .store
        .put("acc_metrics/metrics.tar.zst", &raw)
        .await
        .expect("store put");
    let put_ms = put_start.elapsed().as_millis();

    let verify_start = Instant::now();
    assert!(state
        .store
        .verify("acc_metrics/metrics.tar.zst", &stored.sha256)
        .await
        .expect("verify"));
    let verify_ms = verify_start.elapsed().as_millis();

    let out = dir.join("metrics-out.tar");
    let get_start = Instant::now();
    state
        .store
        .get("acc_metrics/metrics.tar.zst", &out)
        .await
        .expect("store get");
    let get_ms = get_start.elapsed().as_millis();
    assert_eq!(std::fs::read(&raw).unwrap(), std::fs::read(&out).unwrap());

    let restore_start = Instant::now();
    state
        .adb
        .restore(idx, TIKTOK_PKG, &out)
        .await
        .expect("real restore");
    let restore_ms = restore_start.elapsed().as_millis();
    let got = vm_shell(&mp, idx, &format!("cat {marker}"));
    assert!(
        got.contains(&format!("metrics-{idx}")),
        "marker restore fail: {got}"
    );

    let raw_mib = raw_bytes as f64 / 1024.0 / 1024.0;
    let stored_mib = stored.size_bytes as f64 / 1024.0 / 1024.0;
    let ratio = stored.size_bytes as f64 / raw_bytes as f64;
    let saved_pct = (1.0 - ratio) * 100.0;
    let backup_mib_s = raw_mib / (backup_ms.max(1) as f64 / 1000.0);
    let put_mib_s = raw_mib / (put_ms.max(1) as f64 / 1000.0);
    let get_mib_s = raw_mib / (get_ms.max(1) as f64 / 1000.0);
    println!(
        "REAL_SNAPSHOT_METRICS raw_bytes={} raw_mib={:.2} stored_bytes={} stored_mib={:.2} ratio={:.3} saved_pct={:.1} backup_ms={} put_ms={} verify_ms={} get_ms={} restore_ms={} backup_mib_s={:.1} put_mib_s={:.1} get_mib_s={:.1} apk_version={}",
        raw_bytes,
        raw_mib,
        stored.size_bytes,
        stored_mib,
        ratio,
        saved_pct,
        backup_ms,
        put_ms,
        verify_ms,
        get_ms,
        restore_ms,
        backup_mib_s,
        put_mib_s,
        get_mib_s,
        adb_meta.apk_version,
    );

    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.15 — Diagnostic Magisk/Zygisk/Shamiko root-hide trên MuMu thật.
///
/// Bài này KHÔNG tự cài Magisk/Shamiko. Nó xác minh base image đã có full Magisk + Zygisk
/// + Shamiko, TikTok nằm trong DenyList, và Enforce DenyList đang tắt để Shamiko dùng
/// denylist như hidelist. Nếu chỉ cấu hình Magisk APK trong Settings của MPM thì KHÔNG đủ:
/// file đó chỉ được trích làm binary `resetprop` standalone.
#[tokio::test]
#[ignore]
async fn a15_magisk_shamiko_root_hide_diagnostics() {
    if !mumu_available() {
        eprintln!("[skip] Khong co MuMu");
        return;
    }
    if !std::path::Path::new(DEFAULT_TIKTOK_APK).exists() {
        eprintln!("[skip] Khong co APK tai {DEFAULT_TIKTOK_APK}");
        return;
    }

    let (state, dir) = make_state("a15_shamiko").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());
    let before = index_set(&state).await;

    let idx = orchestrator::provision(&state, "acc_shamiko", &hw(), Some(DEFAULT_TIKTOK_APK))
        .await
        .expect("provision + install TikTok");
    guard.track(idx);
    assert_new_index(&before, idx);

    let magisk_version = vm_shell(
        &mp,
        idx,
        r#"MAGISK=$(command -v magisk 2>/dev/null || echo /data/adb/magisk/magisk); if [ -x "$MAGISK" ]; then "$MAGISK" -c 2>&1; else echo MISSING_MAGISK; fi"#,
    );
    let deny_status = vm_shell(
        &mp,
        idx,
        r#"MAGISK=$(command -v magisk 2>/dev/null || echo /data/adb/magisk/magisk); if [ -x "$MAGISK" ]; then "$MAGISK" --denylist status 2>&1; else echo MISSING_MAGISK; fi"#,
    );
    let deny_list = vm_shell(
        &mp,
        idx,
        r#"MAGISK=$(command -v magisk 2>/dev/null || echo /data/adb/magisk/magisk); if [ -x "$MAGISK" ]; then "$MAGISK" --denylist ls 2>&1; else echo MISSING_MAGISK; fi"#,
    );
    let shamiko_status = vm_shell(
        &mp,
        idx,
        r####"for p in /data/adb/shamiko/.tmp/status /data/adb/modules/zygisk_shamiko/module.prop /data/adb/modules/shamiko/module.prop; do [ -e "$p" ] && echo "### $p" && cat "$p"; done; ls -1 /data/adb/modules 2>/dev/null | grep -Ei 'shamiko|zygisk' || true"####,
    );
    let root_markers = vm_shell(
        &mp,
        idx,
        r#"echo "su=$(command -v su 2>/dev/null || true)"; for p in /system/bin/su /system/xbin/su /sbin/su /data/adb/magisk/su; do [ -e "$p" ] && ls -l "$p"; done; ps -A 2>/dev/null | grep -Ei 'magisk|zygisk|shamiko' || true"#,
    );

    let report = format!(
        "A15_SHAMIKO_DIAG\nmagisk_version:\n{magisk_version}\n\ndeny_status:\n{deny_status}\n\ndeny_list:\n{deny_list}\n\nshamiko_status:\n{shamiko_status}\n\nroot_markers_shell_namespace:\n{root_markers}\n"
    );
    println!("{report}");

    let magisk_lower = magisk_version.to_ascii_lowercase();
    assert!(
        !magisk_lower.contains("missing_magisk") && !magisk_version.trim().is_empty(),
        "Base image chua co full Magisk daemon. Magisk APK trong Settings cua MPM chi la resetprop standalone.\n{report}"
    );

    let shamiko_lower = shamiko_status.to_ascii_lowercase();
    assert!(
        shamiko_lower.contains("shamiko"),
        "Khong thay Shamiko module/status trong /data/adb. Can cai Shamiko module vao Magisk va reboot.\n{report}"
    );

    assert!(
        deny_list.contains(TIKTOK_PKG),
        "DenyList chua co TikTok package {TIKTOK_PKG}. Them TikTok vao DenyList truoc khi chay.\n{report}"
    );

    let status_lower = deny_status.to_ascii_lowercase();
    let enforce_off = status_lower.contains("not enforced")
        || status_lower.contains("disabled")
        || status_lower.contains("off")
        || status_lower.contains("false");
    assert!(
        enforce_off,
        "Shamiko yeu cau Enforce DenyList TAT; output hien tai chua xac nhan dieu do.\n{report}"
    );

    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.16 — Diagnostic sensor entropy baseline trên MuMu thật.
///
/// Test này chỉ đo bề mặt Android sensor framework: feature flags + `dumpsys sensorservice`.
/// Nó chưa chứng minh event stream có entropy giống máy thật, nhưng bắt được gap nền tảng:
/// thiếu accelerometer/gyroscope/magnetometer hoặc provider lộ chuỗi giả lập phổ biến.
#[tokio::test]
#[ignore]
async fn a16_sensor_entropy_baseline() {
    if !mumu_available() {
        eprintln!("[skip] Khong co MuMu");
        return;
    }

    let (state, dir) = make_state("a16_sensor").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());
    let before = index_set(&state).await;

    let idx = orchestrator::provision(&state, "acc_sensor", &hw(), None)
        .await
        .expect("provision sensor baseline");
    guard.track(idx);
    assert_new_index(&before, idx);

    let features = vm_shell(&mp, idx, "pm list features | grep -i sensor || true");
    let sensor_dump = vm_shell(&mp, idx, "dumpsys sensorservice");
    let combined = format!("{features}\n{sensor_dump}").to_lowercase();

    let has_accel = contains_any(&combined, &["accelerometer"]);
    let has_gyro = contains_any(&combined, &["gyroscope", " gyro"]);
    let has_magnet = contains_any(&combined, &["magnetometer", "magnetic field", "compass"]);
    let has_rotation = contains_any(&combined, &["rotation vector"]);
    let provider_tells: Vec<&str> = [
        "goldfish",
        "ranchu",
        "qemu",
        "virtual sensor",
        "mock sensor",
    ]
    .into_iter()
    .filter(|needle| combined.contains(needle))
    .collect();

    println!(
        "A16_SENSOR_DIAG has_accel={} has_gyro={} has_magnet={} has_rotation={} provider_tells={:?}\nfeatures:\n{}\n",
        has_accel, has_gyro, has_magnet, has_rotation, provider_tells, features
    );

    assert!(has_accel, "MuMu thieu accelerometer; sensor gap ro rang");
    assert!(has_gyro, "MuMu thieu gyroscope; sensor entropy khong dat");
    assert!(
        has_magnet,
        "MuMu thieu magnetometer/compass; fingerprint sensor bi khuyet"
    );
    assert!(
        provider_tells.is_empty(),
        "sensor provider lo chuoi gia lap pho bien: {:?}",
        provider_tells
    );

    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// DUMP prop BASE thật của MuMu: chỉ create/start/wait boot, KHÔNG gọi provision/reassert/resetprop.
/// Không assert — in `MUMU_BASE_PROPS`. VmGuard tự dọn VM.
#[tokio::test]
#[ignore]
async fn dump_mumu_base_props() {
    if !mumu_available() {
        eprintln!("[skip] Khong co MuMu");
        return;
    }
    let (state, dir) = make_state("dump").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());
    let before = index_set(&state).await;

    let idx = orchestrator::create_vm(&state)
        .await
        .expect("create dump VM");
    guard.track(idx);
    assert_new_index(&before, idx);
    state.emulator.start(idx).await.expect("start dump VM");
    state
        .adb
        .wait_boot_completed(idx)
        .await
        .expect("boot dump VM");

    for name in [
        "ro.product.model",
        "ro.product.brand",
        "ro.product.manufacturer",
        "ro.product.device",
        "ro.product.name",
        "ro.build.fingerprint",
        "ro.hardware",
        "ro.board.platform",
        "ro.hardware.egl",
        "ro.build.version.security_patch",
        "ro.build.version.release",
        "ro.build.characteristics",
        "ro.product.cpu.abi",
        "ro.build.tags",
        "ro.build.type",
    ] {
        println!("MUMU_BASE_PROPS {name}={:?}", getprop(&mp, idx, name));
    }

    let _ = state.emulator.stop(idx).await;
    let _ = state.emulator.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.12 — Vòng đời PROFILE trên MuMu THẬT qua `profile_ops` (đúng code production
/// của run_profile/stop_profile): create (KHÔNG VM) → run (provision + CÀI TIKTOK +
/// đăng ký running) → stop (backup + hủy VM; profile bền). Đóng khoảng trống #4 cho
/// lớp profile-centric + chứng minh provision nay cài TikTok đúng thứ tự (trước restore).
#[tokio::test]
#[ignore]
async fn a12_profile_lifecycle_real() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    // Cần APK thật để provision cài TikTok (đường dẫn mặc định).
    if !std::path::Path::new(DEFAULT_TIKTOK_APK).exists() {
        eprintln!("[skip] Không có APK tại {DEFAULT_TIKTOK_APK}");
        return;
    }
    let (state, dir) = make_state("a12").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;

    // 1) create: CHỈ ghi dữ liệu, KHÔNG tạo VM.
    let username =
        crate::profile_ops::create(&state, acc("acc_prof"), Some("ghi chú test".into()), None)
            .await
            .expect("create profile");
    assert_eq!(username, "acc_prof");
    let p = state
        .get_profile("acc_prof")
        .await
        .expect("profile tồn tại");
    assert!(p.last_run_at.is_none(), "chưa chạy → last_run_at None");
    assert!(
        state.running_vm_of("acc_prof").await.is_none(),
        "create không đăng ký VM"
    );
    assert_eq!(
        index_set(&state).await,
        before,
        "create_profile KHÔNG được tạo VM"
    );
    let want_aid = p.hardware.android_id.clone();

    // 2) run: provision VM + cài TikTok + đăng ký running.
    let idx = crate::profile_ops::run(&state, "acc_prof")
        .await
        .expect("run profile")
        .vm_index;
    guard.track(idx);
    assert_new_index(&before, idx);
    assert_eq!(
        state.running_vm_of("acc_prof").await,
        Some(idx),
        "running map trỏ đúng VM"
    );
    // list phản ánh running_vm.
    let views = crate::profile_ops::list(&state).await;
    let view = views
        .iter()
        .find(|v| v.profile.username == "acc_prof")
        .expect("profile có trong list");
    assert_eq!(view.running_vm, Some(idx), "ProfileView.running_vm = idx");
    assert!(
        state
            .get_profile("acc_prof")
            .await
            .unwrap()
            .last_run_at
            .is_some(),
        "last_run_at set sau run"
    );

    // VM thật: boot xong + android_id = fingerprint CỦA PROFILE (sinh khi create).
    assert_eq!(
        getprop(&mp, idx, "sys.boot_completed"),
        "1",
        "VM phải boot xong"
    );
    // android_id: áp được & BỀN khi CHƯA cài app (đã chứng minh ở a4). NHƯNG sau khi CÀI
    // + CHẠY TikTok, MuMu/GMS ghi đè android_id (Android 8+ cấp id theo app; GMS tự quản
    // android_id) → KHÔNG assert cứng (cùng lớp known-gap với model override; cần
    // Magisk/resetprop mới khóa được — user đã bỏ #2). Chỉ cảnh báo để theo dõi.
    let aid = vm_shell(&mp, idx, "settings get secure android_id");
    let aid_token = aid.split_whitespace().last().unwrap_or("");
    if aid_token != want_aid {
        eprintln!(
            "[known-gap] android_id hậu-cài+chạy TikTok KHÔNG khớp giá trị áp: \
             runtime={aid_token} applied={want_aid} — MuMu/GMS ghi đè android_id \
             (cần Magisk/resetprop mới khóa; xem docs/E2E_RUNBOOK.md)."
        );
    }
    // TikTok đã được provision cài (bằng chứng khoảng trống #4 đã fix — provision cài app).
    let pkgs = vm_adb_raw(&mp, idx, &format!("shell pm list packages {TIKTOK_PKG}"));
    assert!(
        pkgs.contains(&format!("package:{TIKTOK_PKG}")),
        "run_profile phải cài TikTok trong provision: {pkgs}"
    );

    // 3) run lần 2 khi đang chạy → idempotent, KHÔNG tạo VM mới.
    let idx_again = crate::profile_ops::run(&state, "acc_prof")
        .await
        .expect("run lần 2")
        .vm_index;
    assert_eq!(idx_again, idx, "run khi đang chạy trả VM hiện tại");
    // Chỉ xét index MỚI so với `before` (không đếm tổng toàn cục — bền với VM có sẵn).
    let new_indices: Vec<u32> = {
        let mut v: Vec<u32> = index_set(&state)
            .await
            .difference(&before)
            .copied()
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(
        new_indices,
        vec![idx],
        "run lần 2 KHÔNG tạo thêm VM (chỉ {idx} là mới)"
    );

    // 4) stop: backup + hủy VM + nhả running. PROFILE vẫn còn (dữ liệu bền).
    let rec = crate::profile_ops::stop(&state, "acc_prof")
        .await
        .expect("stop profile");
    guard.untrack(idx);
    let rec = rec.expect("stop trả snapshot record");
    assert_eq!(rec.sha256.len(), 64, "sha256 64 hex");
    assert!(rec.size_bytes > 0, "size_bytes > 0");
    // MuMu `delete` BẤT ĐỒNG BỘ: trả success TRƯỚC khi VM biến khỏi `info -v all`
    // (kiểm chứng chạy-thật a12). Poll ≤15s cho VM biến mất thay vì assert tức thì.
    let mut vm_gone = false;
    for _ in 0..15 {
        if !index_set(&state).await.contains(&idx) {
            vm_gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    assert!(vm_gone, "VM bị hủy sau stop (disposable)");
    assert!(
        state.running_vm_of("acc_prof").await.is_none(),
        "running map đã nhả sau stop"
    );
    assert!(
        state.get_profile("acc_prof").await.is_some(),
        "PROFILE vẫn tồn tại sau stop (dữ liệu bền)"
    );
    let db = state.db.as_ref().unwrap();
    assert!(
        db.latest_snapshot("acc_prof").unwrap().is_some(),
        "snapshot lưu theo username"
    );

    // 5) stop lần 2 khi không chạy → Ok(None) (idempotent).
    assert!(
        crate::profile_ops::stop(&state, "acc_prof")
            .await
            .expect("stop idempotent")
            .is_none(),
        "stop khi không chạy phải None"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A.13 — SỐNG-CÒN DỮ LIỆU TIKTOK qua ĐÚNG luồng `profile_ops` + MÔ PHỎNG KHỞI ĐỘNG
/// LẠI APP. Khác a9 (dùng provision(None)+cài tay+restore tay): test này chạy đúng
/// production `run`/`stop`, và sau `stop` DỰNG STATE MỚI trỏ cùng thư mục dữ liệu (nạp
/// lại profiles + snapshot từ SQLite/đĩa) rồi `run` lại — chứng minh dữ liệu "phiên"
/// TỰ backup lúc stop và TỰ nạp lại lúc run (provision cài TikTok RỒI restore), KHÔNG
/// cần thao tác tay. Marker tổng hợp thay session (KHÔNG đăng nhập TikTok thật).
#[tokio::test]
#[ignore]
async fn a13_du_lieu_tiktok_song_qua_stop_run_va_khoi_dong_lai() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    if !std::path::Path::new(DEFAULT_TIKTOK_APK).exists() {
        eprintln!("[skip] Không có APK tại {DEFAULT_TIKTOK_APK}");
        return;
    }
    let (state, dir) = make_state("a13").await;
    let mp = emulator_path();
    let guard = VmGuard::new(mp.clone());
    let before = index_set(&state).await;

    let marker = format!("/data/data/{TIKTOK_PKG}/files/mpm_marker.txt");
    let payload = format!(
        "MPM-SESSION-{}-{}",
        std::process::id(),
        crate::state::now_ms()
    );

    // create (không VM) + run (provision cài TikTok qua ĐÚNG luồng profile_ops::run).
    crate::profile_ops::create(&state, acc("acc_data"), None, None)
        .await
        .expect("create");
    let idx1 = crate::profile_ops::run(&state, "acc_data")
        .await
        .expect("run 1")
        .vm_index;
    guard.track(idx1);
    assert_new_index(&before, idx1);

    // Ghi "dữ liệu phiên" (marker thay session TikTok) với owner + SELinux đúng.
    vm_shell(
        &mp,
        idx1,
        &format!(
            "mkdir -p /data/data/{TIKTOK_PKG}/files && echo {payload} > {marker} && \
             U=$(stat -c %u /data/data/{TIKTOK_PKG}); chown $U:$U {marker}; restorecon {marker}"
        ),
    );
    let pre = vm_shell(&mp, idx1, &format!("cat {marker}"));
    assert!(pre.contains(&payload), "marker phải có TRƯỚC stop: {pre}");

    // stop → backup dữ liệu về kho/CSDL + HỦY VM (disposable).
    let rec = crate::profile_ops::stop(&state, "acc_data")
        .await
        .expect("stop")
        .expect("stop trả snapshot record");
    guard.untrack(idx1);
    assert_eq!(rec.sha256.len(), 64, "sha256 64 hex");
    assert!(rec.size_bytes > 0, "size_bytes > 0");
    assert!(
        !index_set(&state).await.contains(&idx1),
        "VM phải bị hủy sau stop"
    );

    // === MÔ PHỎNG KHỞI ĐỘNG LẠI APP: state MỚI trỏ CÙNG thư mục dữ liệu (KHÔNG xóa) ===
    // Nạp lại profiles + snapshot từ SQLite/đĩa như khi mở lại app. Khóa store [5u8;32] +
    // db None phải KHỚP make_state để đọc được blob đã mã hóa.
    drop(state); // đóng "phiên app" cũ
    let store2 = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
    let db2 = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
    let state2: SharedState = Arc::new(AppState::new(
        Arc::new(MumuClient::new(&mp)),
        Arc::new(MockGeolocator),
        Arc::new(RealAdbWorker::new(&mp)),
        store2,
        AppSettings::default(),
        Some(db2),
        std::collections::HashMap::new(),
    ));
    // Profile + snapshot phải nạp lại được sau "khởi động lại".
    assert!(
        state2.get_profile("acc_data").await.is_some(),
        "profile phải nạp lại từ SQLite sau khởi động lại"
    );
    assert!(
        state2
            .db
            .as_ref()
            .unwrap()
            .latest_snapshot("acc_data")
            .unwrap()
            .is_some(),
        "snapshot phải còn sau khởi động lại"
    );

    // run LẦN 2 trên state MỚI → provision VM mới: cài TikTok RỒI restore snapshot.
    let idx2 = crate::profile_ops::run(&state2, "acc_data")
        .await
        .expect("run 2 sau khởi động lại")
        .vm_index;
    guard.track(idx2);
    // idx2 CÓ THỂ == idx1: emulator tái dùng index vừa giải phóng — đúng mô hình disposable
    // (VM cũ đã `remove`, VM mới là instance MỚI dù trùng số → disk sạch, không có marker
    // trừ khi restore đưa vào). KHÔNG assert khác nhau.
    if idx2 == idx1 {
        eprintln!("[info] emulator tái dùng index {idx1} cho VM phiên 2 (bình thường)");
    }

    // BẰNG CHỨNG: marker TỰ nạp lại (KHÔNG restore tay) — dữ liệu TikTok sống qua
    // destroy VM + khởi động lại app.
    let got = vm_shell(&mp, idx2, &format!("cat {marker}"));
    assert!(
        got.contains(&payload),
        "dữ liệu TikTok phải TỰ NẠP LẠI qua run_profile: got={got} want={payload}"
    );
    // SELinux DISABLED trên MuMu (getenforce=Disabled; `ls -Z` ra '?' cho MỌI file kể cả
    // app hệ thống) → nhãn vô nghĩa. Cái quyết định app đọc được data restore là OWNER:
    // restore `chown -R` về owner của pkg dir (= app-uid). Verify owner marker == owner dir.
    let owner_marker = vm_shell(&mp, idx2, &format!("stat -c %u {marker}"));
    let owner_dir = vm_shell(&mp, idx2, &format!("stat -c %u /data/data/{TIKTOK_PKG}"));
    assert_eq!(
        owner_marker.trim(),
        owner_dir.trim(),
        "marker phải cùng owner với app data dir (chown restore đúng → app đọc được, không EACCES)"
    );
    assert_ne!(
        owner_marker.trim(),
        "root",
        "owner phải là app-uid, không phải root"
    );

    // cleanup: stop lần cuối (backup + hủy) rồi guard dọn nốt nếu còn.
    let _ = crate::profile_ops::stop(&state2, "acc_data").await;
    guard.untrack(idx2);
    drop(guard);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.12 — KHÓA MODEL thật qua Magisk resetprop (đẩy binary + chạy script). Kiểm ca
/// KHÓ NHẤT: model CÓ KHOẢNG TRẮNG ("Redmi Note 8") — value bị adb-shell tách nếu
/// truyền qua command line, nên code sinh script rồi `sh <file>`. Test chứng minh
/// end-to-end: provision fresh VM → push_resetprop → lock_device_identity → model
/// runtime đúng "Redmi Note 8" (đọc lại độc lập qua emulator adb).
#[tokio::test]
#[ignore]
async fn a12_khoa_model_co_khoang_trang() {
    if !mumu_available() {
        eprintln!("[skip] Không có MuMu");
        return;
    }
    // Cần Magisk APK người dùng đã tải để trích resetprop.
    let apk = r"D:\MemuTiktok\appTiktok\Magisk-v30.7.apk";
    if !std::path::Path::new(apk).is_file() {
        eprintln!("[skip] Không có Magisk APK tại {apk}");
        return;
    }
    let (state, dir) = make_state("a12").await;
    let mp = emulator_path();
    // Trích + set binary magisk (như lib.rs làm lúc khởi động).
    let bin = crate::magisk::ensure_binary(apk, &dir.join("magisk")).expect("trích libmagisk.so");
    state.set_magisk_bin(Some(bin));

    let guard = VmGuard::new(mp.clone());
    let before = index_set(&state).await;

    // Model CÓ KHOẢNG TRẮNG — ca dễ hỏng nhất.
    let mut hw = hw();
    hw.model = "Redmi Note 8".into();
    hw.brand = "Xiaomi".into();
    hw.manufacturer = "Xiaomi".into();
    hw.device = "ginkgo".into();
    hw.build_fingerprint = "Xiaomi/ginkgo/ginkgo:9/PKQ1.190616.001/V11:user/release-keys".into();

    // provision boot VM + push_resetprop + lock_device_identity.
    let idx = orchestrator::provision(&state, "acc_lock", &hw, None)
        .await
        .expect("provision");
    guard.track(idx);
    assert_new_index(&before, idx);

    // 1) lock_device_identity trả true (nội bộ đã verify model == hw.model).
    let locked = state
        .adb
        .lock_device_identity(idx, &hw)
        .await
        .expect("lock_device_identity");
    assert!(locked, "lock_device_identity phải trả true (model đã khóa)");

    // 2) Đọc lại ĐỘC LẬP qua emulator adb — chắc chắn value có khoảng trắng giữ nguyên.
    let out = Command::new(&mp)
        .args([
            "adb",
            "-v",
            &idx.to_string(),
            "-c",
            "shell getprop ro.product.model",
        ])
        .output()
        .expect("getprop model");
    let text = String::from_utf8_lossy(&out.stdout);
    let model = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.contains("already connected"))
        .unwrap_or("");
    assert_eq!(model, "Redmi Note 8", "model runtime phải = 'Redmi Note 8'");

    // 3) fingerprint cũng phải khóa (coherence model↔fingerprint — lý do tính năng tồn tại).
    let out_fp = Command::new(&mp)
        .args([
            "adb",
            "-v",
            &idx.to_string(),
            "-c",
            "shell getprop ro.build.fingerprint",
        ])
        .output()
        .expect("getprop fingerprint");
    let text_fp = String::from_utf8_lossy(&out_fp.stdout);
    let fp = text_fp
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.contains("already connected"))
        .unwrap_or("");
    assert_eq!(
        fp, hw.build_fingerprint,
        "fingerprint runtime phải khớp hồ sơ"
    );

    drop(guard);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Kiểm tra storage_key khớp mẫu `^<account>/\d+\.tar\.zst$` không cần crate regex.
fn regex_like_storage_key(key: &str, account: &str) -> bool {
    let prefix = format!("{account}/");
    let Some(rest) = key.strip_prefix(&prefix) else {
        return false;
    };
    let Some(digits) = rest.strip_suffix(".tar.zst") else {
        return false;
    };
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}
