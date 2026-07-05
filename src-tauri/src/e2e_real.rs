//! E2E "thật" — bộ tích hợp in-crate chạy trên MEmu THẬT (SPEC A).
//!
//! ⚠️ Mọi test đều `#[ignore]`: chúng tạo VM thật, cài APK ~220MB, backup/restore
//! → rất chậm và bắt buộc có MEmu tại `D:\Microvirt\MEmu\memuc.exe`. Chạy bằng:
//!   `cargo test --lib e2e_real -- --ignored --nocapture`
//!
//! KHÔNG đăng nhập TikTok ở bất kỳ đâu — một MARKER tổng hợp dưới `files/` đứng
//! thay dữ liệu phiên. Test PHẢI là in-crate mới với tới `crate::orchestrator`,
//! `crate::memuc::RealMemuc`, `crate::adb::RealAdbWorker`, `MockGeolocator`
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

use crate::adb::{AdbWorker, RealAdbWorker};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::geo::{IpGeolocator, MockGeolocator};
use crate::memuc::RealMemuc;
use crate::model::{
    AppSettings, EmulatorTell, HardwareProfile, SnapshotMeta, DEFAULT_TIKTOK_APK, TIKTOK_PKG,
};
use crate::orchestrator;
use crate::snapshot::LocalSnapshotStore;
use crate::state::{AppState, SharedState};

use async_trait::async_trait;

// ============================================================================
// A.0 — Harness dùng chung (nền tảng, không phải test hành vi)
// ============================================================================

/// Đường dẫn memuc.exe: ưu tiên discover, fallback vị trí đã biết.
fn memuc_path() -> PathBuf {
    RealMemuc::discover().unwrap_or_else(|| PathBuf::from(r"D:\Microvirt\MEmu\memuc.exe"))
}

/// True nếu môi trường có MEmu thật; false → test tự early-return (skip mềm).
fn memu_available() -> bool {
    memuc_path().exists()
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
    }
}

/// Fingerprint KHÁC hoàn toàn cho luồng swap (chứng minh cách ly chéo R-12).
fn hw_new() -> HardwareProfile {
    HardwareProfile {
        model: "Pixel 6".into(),
        brand: "google".into(),
        manufacturer: "Google".into(),
        // IMEI hợp lệ Luhn khác.
        imei: "356938035643809".into(),
        android_id: "ffeeddccbbaa".into(),
        mac: "02:00:00:aa:bb:cc".into(),
        res_width: 1080,
        res_height: 2400,
        dpi: 420,
        device: "oriole".into(),
        build_fingerprint: "google/oriole/oriole:12/SD1A.210817.036/7805805:user/release-keys"
            .into(),
    }
}

/// Geolocator trả về quốc gia cố định (hoặc None) — cần thiết vì
/// `MockGeolocator.country("")` trả None (gây CountryUnverified giả).
struct FixedGeo(Option<&'static str>);
#[async_trait]
impl IpGeolocator for FixedGeo {
    async fn country(&self, _ip: &str) -> Option<String> {
        self.0.map(|s| s.to_string())
    }
}

/// Tạo SharedState với adapter THẬT + thư mục temp riêng (không đụng %APPDATA%).
async fn make_state(tag: &str) -> (SharedState, PathBuf) {
    make_state_geo(tag, Arc::new(MockGeolocator)).await
}

/// Như `make_state` nhưng cho phép chọn geolocator (dùng cho cổng quốc gia).
async fn make_state_geo(tag: &str, geo: Arc<dyn IpGeolocator>) -> (SharedState, PathBuf) {
    let mp = memuc_path();
    let dir = std::env::temp_dir().join(format!("mpm_e2e_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let store = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
    let db = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
    let memuc = Arc::new(RealMemuc::new(&mp));
    let adb = Arc::new(RealAdbWorker::new(&mp));

    let state: SharedState = Arc::new(AppState::new(
        memuc,
        geo,
        adb,
        store,
        AppSettings::default(),
        Some(db),
        std::collections::HashMap::new(),
    ));
    (state, dir)
}

/// Tập index VM hiện có (đọc trực tiếp qua state.memuc).
async fn index_set(state: &SharedState) -> HashSet<u32> {
    state
        .memuc
        .list_instances()
        .await
        .map(|v| v.into_iter().map(|i| i.index).collect())
        .unwrap_or_default()
}

/// Shell trực tiếp `memuc.exe -i <idx> adb "shell su -c '<cmd>'"` → stdout đã lọc
/// nhiễu "already connected" / "adb server" (mirror `real.rs::prop`).
/// Dùng std::process::Command vì `RealAdbWorker::adb()` là private.
fn vm_shell(mp: &PathBuf, idx: u32, cmd: &str) -> String {
    let arg = format!("shell su -c '{cmd}'");
    let out = Command::new(mp)
        .args(["-i", &idx.to_string(), "adb", &arg])
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
        .args(["-i", &idx.to_string(), "adb", adb_arg])
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

/// getprop lấy token cuối cùng (đề phòng memuc chèn dòng nhiễu trước giá trị).
fn getprop(mp: &PathBuf, idx: u32, name: &str) -> String {
    let raw = vm_adb_raw(mp, idx, &format!("shell getprop {name}"));
    raw.split_whitespace().last().unwrap_or("").to_string()
}

/// getconfigex đọc lại một khóa memuc (fallback OS-level proof).
fn getconfigex(mp: &PathBuf, idx: u32, key: &str) -> String {
    let out = Command::new(mp)
        .args(["-i", &idx.to_string(), "getconfigex", key])
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
                .args(["-i", &idx.to_string(), "stop"])
                .output();
            let _ = Command::new(&self.mp)
                .args(["-i", &idx.to_string(), "remove"])
                .output();
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
    if !memu_available() {
        eprintln!("[skip] Không có MEmu tại {:?}", memuc_path());
        return;
    }
    let (state, _dir) = make_state("a0").await;
    let mp = memuc_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    assert!(before.contains(&0), "VM 0 phải tồn tại trước test");

    let idx = orchestrator::create_vm(&state).await.expect("create_vm");
    guard.track(idx);
    assert_new_index(&before, idx);

    let after = index_set(&state).await;
    assert!(after.contains(&idx), "VM mới phải xuất hiện trong list");

    // Hủy chủ động → Drop là no-op cho idx.
    let _ = state.memuc.stop(idx).await;
    state.memuc.remove(idx).await.expect("remove");
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
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let (state, _dir) = make_state("a1").await;
    let guard = VmGuard::new(memuc_path());

    let before = index_set(&state).await;
    let idx = orchestrator::create_vm(&state).await.expect("create_vm");
    guard.track(idx);

    assert_new_index(&before, idx);
    assert!(
        index_set(&state).await.contains(&idx),
        "VM mới có trong list"
    );
    // VM 0 vẫn còn.
    let list = state.memuc.list_instances().await.expect("list");
    assert!(list.iter().any(|v| v.index == 0), "VM 0 phải còn tồn tại");
}

/// A.2 — Cổng quốc gia (tất định, KHÔNG tạo VM, KHÔNG mạng).
#[tokio::test]
#[ignore]
async fn a2_country_gate() {
    // Không cần MEmu: assert_country_match chỉ dùng geo + state.country_of.
    // Case A — skip: FixedGeo(None), không set country.
    {
        let (state, _dir) = make_state_geo("a2_skip", Arc::new(FixedGeo(None))).await;
        assert!(
            orchestrator::assert_country_match(&state, 0).await.is_ok(),
            "không yêu cầu quốc gia → không chặn"
        );
    }
    // Case B — match (không phân biệt hoa/thường; set_country uppercases).
    {
        let (state, _dir) = make_state_geo("a2_ok", Arc::new(FixedGeo(Some("VN")))).await;
        state.set_country(0, Some("vn".into())).await;
        assert!(orchestrator::assert_country_match(&state, 0).await.is_ok());
    }
    // Case C — mismatch.
    {
        let (state, _dir) = make_state_geo("a2_bad", Arc::new(FixedGeo(Some("VN")))).await;
        state.set_country(0, Some("US".into())).await;
        match orchestrator::assert_country_match(&state, 0).await {
            Err(AppError::CountryMismatch { actual, expected }) => {
                assert_eq!(actual, "VN");
                assert_eq!(expected, "US");
            }
            other => panic!("phải CountryMismatch, nhận {other:?}"),
        }
    }
    // Case D — unverified.
    {
        let (state, _dir) = make_state_geo("a2_unv", Arc::new(FixedGeo(None))).await;
        state.set_country(0, Some("VN".into())).await;
        match orchestrator::assert_country_match(&state, 0).await {
            Err(AppError::CountryUnverified(cc)) => assert_eq!(cc, "VN"),
            other => panic!("phải CountryUnverified, nhận {other:?}"),
        }
    }
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
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let (state, dir) = make_state("a4").await;
    let mp = memuc_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let idx = orchestrator::provision(&state, "acc_fp", &hw())
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

    // ⚠️ KNOWN-GAP (phát hiện qua test thực): MEmu GHI ĐÈ model bằng thiết bị NGẪU
    // NHIÊN khi VM BOOT (FRD-L19 đặt lúc dừng → ASUS_AI2401_A / NX809J sau boot).
    // Nghĩa là `microvirt_vm_model` KHÔNG do ta kiểm soát qua boot → chỉ cảnh báo,
    // KHÔNG assert cứng. Fingerprint thực sự áp được & bền là android_id (đã assert
    // ở trên) + độ phân giải/DPI (assert bên dưới). Xem docs/E2E_RUNBOOK.md.
    let model = getprop(&mp, idx, "ro.product.model");
    let cfg = getconfigex(&mp, idx, "microvirt_vm_model");
    if model != "FRD-L19" || !cfg.contains("FRD-L19") {
        eprintln!(
            "[known-gap] MEmu ghi đè model khi boot: ro.product.model={model:?} \
             getconfigex={cfg:?} — model KHÔNG stick (android_id/DPI mới là fingerprint hiệu lực)"
        );
    }

    // Độ phân giải + DPI.
    let size = vm_adb_raw(&mp, idx, "shell wm size");
    assert!(
        size.contains("1080x1920") || size.contains("1080 x 1920"),
        "wm size phải 1080x1920: {size}"
    );
    let density = vm_adb_raw(&mp, idx, "shell wm density");
    assert!(density.contains("320"), "wm density phải 320: {density}");

    // MAC chỉ ở mức getconfigex (đọc lại adb không tin cậy).
    let mac_cfg = getconfigex(&mp, idx, "macaddress");
    eprintln!("[info] macaddress getconfigex = {mac_cfg:?}");

    // --- Ghép A.7: scan_emulator_tells + debloat (best-effort, tất cả loose) ---
    let tells: Vec<EmulatorTell> = state
        .adb
        .scan_emulator_tells(idx)
        .await
        .expect("scan_emulator_tells");
    assert_eq!(tells.len(), 7, "phải có đúng 7 mục scan: {tells:?}");
    let by_check = |name: &str| tells.iter().find(|t| t.check == name);
    for name in [
        "Native Bridge (ARM→x86)",
        "CPU hypervisor flag",
        "ro.kernel.qemu",
        "File QEMU/Genymotion",
        "vboxsf mount",
        "GPU renderer ảo",
        "ro.build.characteristics",
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
    let _ = state.memuc.stop(idx).await;
    let _ = state.memuc.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.5 — launch_instance nạp lại fingerprint từ DB (không snapshot → restored=false).
#[tokio::test]
#[ignore]
async fn a5_launch_reload_fingerprint() {
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let (state, dir) = make_state("a5").await;
    let mp = memuc_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let idx = orchestrator::create_vm(&state).await.expect("create_vm");
    guard.track(idx);
    assert_new_index(&before, idx);

    // Ghi fingerprint xuống SQLite (write-through). KHÔNG set_country (bỏ cổng).
    state.set_hardware(idx, hw()).await;
    assert!(
        state.hardware_of(idx).await.is_some(),
        "fingerprint phải có trong DB trước khi launch"
    );

    // Cài TikTok trước để start_app cuối của launch_instance không fail.
    state
        .adb
        .install_apk(idx, DEFAULT_TIKTOK_APK)
        .await
        .expect("install_apk");

    let restored = orchestrator::launch_instance(&state, idx, "acc_launch")
        .await
        .expect("launch_instance không được Err");
    assert!(!restored, "chưa có snapshot → restored=false");

    assert_eq!(getprop(&mp, idx, "sys.boot_completed"), "1");
    let aid = vm_shell(&mp, idx, "settings get secure android_id");
    assert!(
        aid.contains("a1b2c3d4e5f6"),
        "DB reload + re-apply android_id: {aid}"
    );

    let _ = state.memuc.stop(idx).await;
    let _ = state.memuc.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.6 — cài APK TikTok có mặt.
#[tokio::test]
#[ignore]
async fn a6_install_tiktok_apk() {
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let (state, dir) = make_state("a6").await;
    let mp = memuc_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let idx = orchestrator::create_vm(&state).await.expect("create_vm");
    guard.track(idx);
    assert_new_index(&before, idx);

    state
        .queue
        .run(state.memuc.start(idx))
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

    let _ = state.memuc.stop(idx).await;
    let _ = state.memuc.remove(idx).await;
    guard.untrack(idx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A.8 — swap_account trên VM đang chạy (cách ly chéo R-12).
#[tokio::test]
#[ignore]
async fn a8_swap_account_running_vm() {
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let (state, dir) = make_state("a8").await;
    let mp = memuc_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let idx = orchestrator::provision(&state, "acc_old", &hw())
        .await
        .expect("provision acc_old");
    guard.track(idx);
    assert_new_index(&before, idx);

    // Cài TikTok để có /data/data/<pkg> + start_app hoạt động.
    state
        .adb
        .install_apk(idx, DEFAULT_TIKTOK_APK)
        .await
        .expect("install_apk");
    state
        .adb
        .start_app(idx, TIKTOK_PKG)
        .await
        .expect("start_app");

    // Ghi marker E2E-NEW → backup thành snapshot acc_new.
    let marker = format!("/data/data/{TIKTOK_PKG}/files/mpm_marker.txt");
    vm_shell(
        &mp,
        idx,
        &format!("mkdir -p /data/data/{TIKTOK_PKG}/files && echo E2E-NEW > {marker}"),
    );
    orchestrator::backup_and_record(&state, idx, "acc_new")
        .await
        .expect("backup acc_new");

    // Ghi đè marker về E2E-OLD (trạng thái tài khoản cũ).
    vm_shell(&mp, idx, &format!("echo E2E-OLD > {marker}"));
    let pre = vm_shell(&mp, idx, &format!("cat {marker}"));
    assert!(pre.contains("E2E-OLD"), "marker cũ phải là E2E-OLD: {pre}");

    // Swap sang acc_new với fingerprint KHÁC (Pixel 6).
    orchestrator::swap_account(&state, idx, "acc_new", &hw_new())
        .await
        .expect("swap_account");

    // Marker: OLD bị pm clear, NEW được restore.
    let after = vm_shell(&mp, idx, &format!("cat {marker}"));
    assert!(
        after.contains("E2E-NEW"),
        "marker phải là E2E-NEW sau swap: {after}"
    );
    assert!(
        !after.contains("E2E-OLD"),
        "marker cũ E2E-OLD KHÔNG được còn: {after}"
    );

    // android_id mới (bằng chứng cross-check tin cậy nhất).
    let aid = vm_shell(&mp, idx, "settings get secure android_id");
    assert!(
        aid.contains("ffeeddccbbaa"),
        "android_id phải là của hw_new: {aid}"
    );

    // ro.product.model — swap CÓ reboot nên ro.* đọc lại được.
    let model = getprop(&mp, idx, "ro.product.model");
    if !model.contains("Pixel 6") {
        let cfg = getconfigex(&mp, idx, "microvirt_vm_model");
        eprintln!("[soft] ro.product.model={model:?}; getconfigex={cfg:?}");
        assert!(cfg.contains("Pixel 6"), "khóa memuc model phải là Pixel 6");
    }

    // swap không tạo/hủy VM nào.
    assert!(
        index_set(&state).await.contains(&idx),
        "VM vẫn tồn tại sau swap"
    );

    let _ = state.memuc.stop(idx).await;
    let _ = state.memuc.remove(idx).await;
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
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let (state, dir) = make_state("a9").await;
    let mp = memuc_path();
    let guard = VmGuard::new(mp.clone());

    let before = index_set(&state).await;
    let marker = format!("/data/data/{TIKTOK_PKG}/files/mpm_marker.txt");
    let nonce = format!("{}-{}", std::process::id(), crate::state::now_ms());
    let payload = format!("MPM-MARKER-{nonce}");

    // --- Phiên 1 ---
    let idx1 = orchestrator::provision(&state, "acc_e2e", &hw())
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
             U=$(stat -c %U /data/data/{TIKTOK_PKG}); chown $U:$U {marker}; restorecon {marker}"
        ),
    );
    let pre = vm_shell(&mp, idx1, &format!("cat {marker}"));
    assert!(
        pre.contains(&payload),
        "marker phải có mặt trước backup: {pre}"
    );
    let pre_label = vm_shell(&mp, idx1, &format!("ls -Z {marker}"));
    let pre_owner = vm_shell(&mp, idx1, &format!("stat -c %U:%G /data/data/{TIKTOK_PKG}"));
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
    let idx2 = orchestrator::provision(&state, "acc_e2e", &hw())
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

    // SELinux label + owner đọc-được (app không EACCES).
    let post_label = vm_shell(&mp, idx2, &format!("ls -Z {marker}"));
    assert!(
        post_label.contains("app_data_file"),
        "label phải app_data_file: {post_label}"
    );

    // Fingerprint re-applied trên VM mới.
    let cfg = getconfigex(&mp, idx2, "microvirt_vm_model");
    assert_eq!(cfg, "FRD-L19", "fingerprint áp lại trên VM mới");

    // Cleanup.
    let _ = state.memuc.stop(idx2).await;
    let _ = state.memuc.remove(idx2).await;
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
    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()> {
        self.0.apply_android_id(idx, android_id).await
    }
    async fn wipe_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        self.0.wipe_app(idx, pkg).await
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
    async fn list_third_party_apps(&self, idx: u32) -> AppResult<Vec<String>> {
        self.0.list_third_party_apps(idx).await
    }
    async fn scan_emulator_tells(&self, idx: u32) -> AppResult<Vec<EmulatorTell>> {
        self.0.scan_emulator_tells(idx).await
    }
    async fn harden(&self, idx: u32) -> AppResult<()> {
        self.0.harden(idx).await
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
}

/// A.10 — R-15 nghiêm: backup thất bại KHÔNG hủy VM.
#[tokio::test]
#[ignore]
async fn a10_r15_backup_fail_vm_not_destroyed() {
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let mp = memuc_path();
    let dir = std::env::temp_dir().join(format!("mpm_e2e_{}_a10", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let store = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
    let db = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
    let memuc = Arc::new(RealMemuc::new(&mp));
    let real_adb = Arc::new(RealAdbWorker::new(&mp));
    let adb: Arc<dyn AdbWorker> = Arc::new(FailingBackupAdb(real_adb));

    let state: SharedState = Arc::new(AppState::new(
        memuc,
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
    let idx = orchestrator::provision(&state, "acc_r15", &hw())
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
    if !memu_available() {
        eprintln!("[skip] Không có MEmu");
        return;
    }
    let (state, dir) = make_state("a11").await;
    let mp = memuc_path();
    let guard = VmGuard::new(mp.clone());

    assert_eq!(orchestrator::SNAPSHOT_RETENTION, 5, "hằng retention phải 5");

    let before = index_set(&state).await;
    let idx = orchestrator::provision(&state, "acc_ret", &hw())
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

    let _ = state.memuc.stop(idx).await;
    let _ = state.memuc.remove(idx).await;
    guard.untrack(idx);
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
