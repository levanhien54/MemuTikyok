//! Orchestrator "Môi trường dùng một lần" (§3 thiết kế Backup/Restore).
//! Điều phối vòng đời: provision (tạo VM sạch → áp hardware → start → restore)
//! và teardown (backup → đồng bộ → HỦY VM). Bất biến an toàn: chỉ hủy VM sau khi
//! backup + ghi CSDL thành công (R-15).

use std::collections::HashSet;
use std::fs;

use crate::error::{AppError, AppResult};
use crate::model::{HardwareProfile, SnapshotMeta, SnapshotRecord, DEFAULT_BLOAT, TIKTOK_PKG};
use crate::state::{now_ms, SharedState};

/// Tập index VM hiện có (từ memuc).
async fn index_set(state: &SharedState) -> AppResult<HashSet<u32>> {
    Ok(state
        .memuc
        .list_instances()
        .await?
        .into_iter()
        .map(|i| i.index)
        .collect())
}

/// Nhận diện index VM **mới** = index xuất hiện sau khi tạo mà trước đó chưa có.
/// Đáng tin hơn `max(index)` vì memuc có thể **tái dùng** index đã xóa (lấp khoảng
/// trống), khiến max() trỏ nhầm sang VM khác.
async fn identify_new(state: &SharedState, before: &HashSet<u32>) -> AppResult<u32> {
    index_set(state)
        .await?
        .difference(before)
        .copied()
        .max()
        .ok_or_else(|| AppError::CommandFailed("Không xác định được VM vừa tạo".into()))
}

/// Tạo một VM mới và trả về index của nó (an toàn với tái dùng index + đua tranh).
/// Giữ `create_lock` suốt tạo→nhận diện để hai lần tạo song song không chọn nhầm.
pub async fn create_vm(state: &SharedState) -> AppResult<u32> {
    let _guard = state.create_lock.lock().await;
    let before = index_set(state).await?;
    state.queue.run(state.memuc.create()).await?;
    identify_new(state, &before).await
}

/// Clone `base_index` và trả về index VM mới (an toàn với tái dùng index + đua tranh).
pub async fn clone_vm(state: &SharedState, base_index: u32) -> AppResult<u32> {
    let _guard = state.create_lock.lock().await;
    let before = index_set(state).await?;
    state.queue.run(state.memuc.clone_vm(base_index)).await?;
    identify_new(state, &before).await
}

/// Gỡ app thừa + ẩn dấu vết ảo MẶC ĐỊNH (best-effort) khi chuẩn bị VM đã boot.
async fn auto_debloat(state: &SharedState, index: u32) {
    for pkg in DEFAULT_BLOAT {
        let _ = state.adb.disable_app(index, pkg).await;
    }
    // Ẩn dấu vết emulator sửa được (native-bridge/hypervisor là giới hạn x86 — xem docs).
    let _ = state.adb.harden(index).await;
}

/// Áp toàn bộ cấu hình phần cứng (fingerprint) vào VM: các khoá setconfigex +
/// **độ phân giải/DPI** (cửa sổ VM mặc định khớp thiết bị fake). android_id áp riêng (adb).
pub async fn apply_hw_config(
    state: &SharedState,
    index: u32,
    hw: &HardwareProfile,
) -> AppResult<()> {
    for (key, value) in hw.memuc_pairs() {
        state.memuc.set_config(index, key, &value).await?;
    }
    state
        .memuc
        .set_resolution(index, hw.res_width, hw.res_height, hw.dpi)
        .await?;
    Ok(())
}

/// Số snapshot giữ lại cho mỗi tài khoản (retention để rollback + tối ưu dung lượng).
pub const SNAPSHOT_RETENTION: u32 = 5;

/// Backup dữ liệu VM → nén & lưu kho (nguyên tử) → ghi CSDL → dọn bản cũ.
/// Dùng chung cho command backup_instance và teardown (tránh trùng lặp).
pub async fn backup_and_record(
    state: &SharedState,
    index: u32,
    account_key: &str,
) -> AppResult<SnapshotRecord> {
    let created = now_ms();
    let tmp = std::env::temp_dir().join(format!("mpm-backup-{index}-{created}.tar"));

    // 1) Trích xuất dữ liệu (archive thô).
    let adb_meta = state.adb.backup(index, TIKTOK_PKG, &tmp).await?;
    // 2) Nén + lưu nguyên tử; sha256/size là của blob ĐÃ NÉN (toàn vẹn + dung lượng thật).
    let storage_key = format!("{account_key}/{created}.tar.zst");
    let stored = state.store.put(&storage_key, &tmp).await?;
    let _ = std::fs::remove_file(&tmp);

    // 3) Ghi CSDL + retention (dọn blob cũ vượt hạn mức).
    if let Some(db) = &state.db {
        let meta = SnapshotMeta {
            sha256: stored.sha256.clone(),
            size_bytes: stored.size_bytes,
            apk_version: adb_meta.apk_version.clone(),
        };
        db.record_snapshot(account_key, &storage_key, &meta, created)?;
        for old_key in db.snapshots_beyond(account_key, SNAPSHOT_RETENTION)? {
            let _ = state.store.delete(&old_key).await;
        }
        db.prune_snapshots(account_key, SNAPSHOT_RETENTION)?;
    }

    Ok(SnapshotRecord {
        storage_key,
        sha256: stored.sha256,
        size_bytes: stored.size_bytes,
        apk_version: Some(adb_meta.apk_version),
        created_at: created,
    })
}

/// Cấp phát một VM mới cho `account_key`: tạo sạch → áp hardware → start →
/// restore snapshot mới nhất (nếu có). Trả về index VM đã sẵn sàng làm việc.
pub async fn provision(
    state: &SharedState,
    account_key: &str,
    hw: &HardwareProfile,
) -> AppResult<u32> {
    // 1) Tạo VM mới tinh và xác định index của nó (an toàn tái dùng index/đua tranh).
    let index = create_vm(state).await?;

    // 2) Áp hồ sơ phần cứng + độ phân giải (nhất quán fingerprint — R-12).
    apply_hw_config(state, index, hw).await?;

    // 3) Start → chờ boot → áp android_id → gỡ app thừa mặc định.
    state.queue.run(state.memuc.start(index)).await?;
    state.mark_launched(index).await;
    state.adb.wait_boot_completed(index).await?;
    state.adb.apply_android_id(index, &hw.android_id).await?;
    auto_debloat(state, index).await;

    // 4) Restore snapshot mới nhất nếu có (verify sha256 trước).
    if let Some(db) = &state.db {
        if let Some(rec) = db.latest_snapshot(account_key)? {
            if state.store.verify(&rec.storage_key, &rec.sha256).await? {
                let tmp =
                    std::env::temp_dir().join(format!("mpm-prov-{index}-{}.tar.zst", now_ms()));
                state.store.get(&rec.storage_key, &tmp).await?;
                state.adb.restore(index, TIKTOK_PKG, &tmp).await?;
                let _ = fs::remove_file(&tmp);
            } else {
                tracing::warn!(account_key, "Snapshot hỏng — provision không restore");
            }
        }
    }

    Ok(index)
}

/// Clone từ **base image** (đã debloat + TikTok + config) rồi áp fingerprint RIÊNG
/// cho tài khoản → nhanh hơn create + có sẵn app. Lưu fingerprint vào DB. Trả index mới.
pub async fn clone_from_base(
    state: &SharedState,
    base_index: u32,
    account_key: &str,
    hw: &HardwareProfile,
) -> AppResult<u32> {
    let index = clone_vm(state, base_index).await?;

    // Lưu fingerprint cho VM mới (để nạp lại lần sau) + áp ngay (gồm độ phân giải).
    state.set_hardware(index, hw.clone()).await;
    apply_hw_config(state, index, hw).await?;

    state.queue.run(state.memuc.start(index)).await?;
    state.mark_launched(index).await;
    state.adb.wait_boot_completed(index).await?;
    state.adb.apply_android_id(index, &hw.android_id).await?;
    auto_debloat(state, index).await;

    if let Some(db) = &state.db {
        if let Some(rec) = db.latest_snapshot(account_key)? {
            if state.store.verify(&rec.storage_key, &rec.sha256).await? {
                let tmp =
                    std::env::temp_dir().join(format!("mpm-clone-{index}-{}.tar.zst", now_ms()));
                state.store.get(&rec.storage_key, &tmp).await?;
                state.adb.restore(index, TIKTOK_PKG, &tmp).await?;
                let _ = fs::remove_file(&tmp);
            }
        }
    }
    state.adb.start_app(index, TIKTOK_PKG).await?;
    Ok(index)
}

/// Nạp **warm pool** tới `target` VM: clone base + boot sẵn (chờ trước, dùng sau →
/// 0s cold-boot khi cần account mới). Trả về số VM hiện có trong pool.
pub async fn pool_refill(state: &SharedState, base_index: u32, target: usize) -> AppResult<usize> {
    while state.pool.lock().await.len() < target {
        let index = clone_vm(state, base_index).await?;
        state.memuc.set_config(index, "enable_su", "1").await?;
        state.queue.run(state.memuc.start(index)).await?;
        state.adb.wait_boot_completed(index).await?;
        auto_debloat(state, index).await;
        state.pool.lock().await.push_back(index);
    }
    Ok(state.pool.lock().await.len())
}

/// Lấy 1 VM **nóng** từ pool gán cho tài khoản (áp fingerprint riêng qua swap).
/// Pool rỗng → fallback `clone_from_base`. Trả về index VM sẵn sàng.
pub async fn pool_acquire(
    state: &SharedState,
    base_index: u32,
    account_key: &str,
    hw: &HardwareProfile,
) -> AppResult<u32> {
    let warm = { state.pool.lock().await.pop_front() };
    match warm {
        Some(index) => {
            swap_account(state, index, account_key, hw).await?;
            Ok(index)
        }
        None => clone_from_base(state, base_index, account_key, hw).await,
    }
}

pub async fn pool_size(state: &SharedState) -> usize {
    state.pool.lock().await.len()
}

/// Vòng lặp nền giữ warm pool luôn đủ `target` (opt-in qua settings; 0 = tắt).
/// Chỉ chạy khi có `pool_base_index` và `warm_pool_target > 0`.
pub async fn pool_maintainer(state: SharedState) {
    use tokio::time::{interval, Duration};
    let mut ticker = interval(Duration::from_secs(30));
    loop {
        ticker.tick().await;
        let (target, base) = {
            let s = state.settings.lock().await;
            (s.warm_pool_target as usize, s.pool_base_index)
        };
        if target == 0 {
            continue;
        }
        let Some(base) = base else { continue };
        if let Err(e) = pool_refill(&state, base, target).await {
            tracing::warn!(error = %e, "Warm pool refill nền thất bại");
        }
    }
}

/// Khởi chạy một VM có sẵn: **NẠP LẠI fingerprint đã lưu trong CSDL và áp** trước
/// khi start (đúng yêu cầu: fingerprint lưu DB, nạp lại khi khởi chạy) → start →
/// chờ boot → android_id → restore session → mở app. Trả về true nếu đã restore.
/// Cổng kiểm tra quốc gia (FR): chỉ cho khởi chạy khi quốc gia IP thoát THỰC TẾ
/// khớp quốc gia YÊU CẦU đã lưu trong CSDL. Bỏ qua nếu VM chưa đặt quốc gia yêu cầu.
///
/// Đã bỏ proxy per-VM → VM thoát mạng qua NAT của host, nên "IP thoát" = IP công
/// khai của host (geolocator tự tra khi truyền IP rỗng). Nếu về sau có proxy/VPN
/// ở tầng host thì IP công khai đã phản ánh đúng, logic không đổi.
pub async fn assert_country_match(state: &SharedState, index: u32) -> AppResult<()> {
    let Some(expected) = state.country_of(index).await else {
        return Ok(()); // không yêu cầu quốc gia → không chặn
    };
    let expected = expected.trim().to_uppercase();
    if expected.is_empty() {
        return Ok(());
    }
    match state.geo.country("").await {
        Some(actual) if actual.eq_ignore_ascii_case(&expected) => Ok(()),
        Some(actual) => Err(AppError::CountryMismatch {
            actual: actual.to_uppercase(),
            expected,
        }),
        None => Err(AppError::CountryUnverified(expected)),
    }
}

pub async fn launch_instance(
    state: &SharedState,
    index: u32,
    account_key: &str,
) -> AppResult<bool> {
    // Cổng quốc gia TRƯỚC khi start (tránh khởi chạy vô ích nếu lệch định vị).
    assert_country_match(state, index).await?;

    let hw = state.hardware_of(index).await;

    // Áp fingerprint + độ phân giải (khi VM còn dừng → ăn khi boot).
    if let Some(hw) = &hw {
        apply_hw_config(state, index, hw).await?;
    }

    state.queue.run(state.memuc.start(index)).await?;
    state.mark_launched(index).await;

    // LUÔN chờ Android boot xong trước mọi lệnh adb (restore/start_app) — kể cả khi
    // không có fingerprint — nếu không adb sẽ chạy lúc thiết bị chưa sẵn sàng.
    state.adb.wait_boot_completed(index).await?;

    if let Some(hw) = &hw {
        state.adb.apply_android_id(index, &hw.android_id).await?;
    }

    // Restore session tài khoản (nếu có; verify sha256).
    let mut restored = false;
    if let Some(db) = &state.db {
        if let Some(rec) = db.latest_snapshot(account_key)? {
            if state.store.verify(&rec.storage_key, &rec.sha256).await? {
                let tmp =
                    std::env::temp_dir().join(format!("mpm-launch-{index}-{}.tar.zst", now_ms()));
                state.store.get(&rec.storage_key, &tmp).await?;
                state.adb.restore(index, TIKTOK_PKG, &tmp).await?;
                let _ = fs::remove_file(&tmp);
                restored = true;
            }
        }
    }

    state.adb.start_app(index, TIKTOK_PKG).await?;
    Ok(restored)
}

/// Kết thúc phiên: backup dữ liệu về kho + CSDL, rồi HỦY VM (disposable).
/// Chỉ hủy sau khi backup thành công (R-15).
pub async fn teardown(
    state: &SharedState,
    index: u32,
    account_key: &str,
) -> AppResult<SnapshotRecord> {
    // 1) Backup (bắt buộc thành công trước khi hủy — R-15).
    let record = backup_and_record(state, index, account_key).await?;

    // 2) Hủy VM (best-effort stop rồi remove) + quên metadata cục bộ.
    let _ = state.memuc.stop(index).await;
    state.memuc.remove(index).await?;
    state.forget(index).await;

    Ok(record)
}

/// Đổi tài khoản trên MỘT VM đang chạy (nhanh hơn tạo/hủy VM — theo yêu cầu tối ưu).
/// Flash sạch app → áp fingerprint RIÊNG cho account mới → reboot (để ro.* ăn) →
/// chờ boot → restore session → mở app. Fingerprint riêng chống liên kết chéo (R-12).
pub async fn swap_account(
    state: &SharedState,
    index: u32,
    account_key: &str,
    hw: &HardwareProfile,
) -> AppResult<()> {
    // 1) Flash sạch dữ liệu app hiện tại (đăng xuất tài khoản cũ).
    state.adb.wipe_app(index, TIKTOK_PKG).await?;

    // 2) Áp hồ sơ phần cứng + độ phân giải riêng cho tài khoản mới.
    apply_hw_config(state, index, hw).await?;

    // 3) Reboot để model/fingerprint (ro.*) có hiệu lực.
    state.queue.run(state.memuc.reboot(index)).await?;
    state.mark_launched(index).await;

    // 4) Chờ Android boot xong (thay sleep cố định → nhanh & chắc).
    state.adb.wait_boot_completed(index).await?;

    // 5) android_id (runtime) sau khi boot.
    state.adb.apply_android_id(index, &hw.android_id).await?;

    // 6) Restore session tài khoản mới (nếu có; verify sha256 trước).
    if let Some(db) = &state.db {
        if let Some(rec) = db.latest_snapshot(account_key)? {
            if state.store.verify(&rec.storage_key, &rec.sha256).await? {
                let tmp =
                    std::env::temp_dir().join(format!("mpm-swap-{index}-{}.tar.zst", now_ms()));
                state.store.get(&rec.storage_key, &tmp).await?;
                state.adb.restore(index, TIKTOK_PKG, &tmp).await?;
                let _ = fs::remove_file(&tmp);
            }
        }
    }

    // 7) Mở TikTok với tài khoản mới.
    state.adb.start_app(index, TIKTOK_PKG).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::MockGeolocator;
    use crate::memuc::{MemucClient, MockMemuc};
    use crate::snapshot::LocalSnapshotStore;
    use crate::state::AppState;
    use crate::{adb::MockAdbWorker, db::Db, model::AppSettings};
    use std::collections::HashMap;
    use std::sync::Arc;

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

    /// Geolocator trả về quốc gia cố định (hoặc None) để test cổng quốc gia.
    struct FixedGeo(Option<&'static str>);
    #[async_trait::async_trait]
    impl crate::geo::IpGeolocator for FixedGeo {
        async fn country(&self, _ip: &str) -> Option<String> {
            self.0.map(|s| s.to_string())
        }
    }

    fn make_state(tag: &str) -> (SharedState, Arc<MockMemuc>, Arc<MockAdbWorker>) {
        make_state_geo(tag, Arc::new(MockGeolocator))
    }

    fn make_state_geo(
        tag: &str,
        geo: Arc<dyn crate::geo::IpGeolocator>,
    ) -> (SharedState, Arc<MockMemuc>, Arc<MockAdbWorker>) {
        let memuc = Arc::new(MockMemuc::new());
        let adb = Arc::new(MockAdbWorker::new());
        let dir = std::env::temp_dir().join(format!("mpm_orch_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
        let db = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
        let state: SharedState = Arc::new(AppState::new(
            memuc.clone(),
            geo,
            adb.clone(),
            store,
            AppSettings::default(),
            Some(db),
            HashMap::new(),
        ));
        (state, memuc, adb)
    }

    #[tokio::test]
    async fn create_vm_nhan_dien_dung_khi_tai_dung_index() {
        // Mock bắt đầu {0,1}. Xóa 0 → còn {1}. Tạo mới → memuc TÁI DÙNG index 0.
        // Phải nhận diện đúng index mới (0) bằng phép hiệu tập, KHÔNG phải max (1).
        let (state, memuc, _adb) = make_state("reuse");
        memuc.remove(0).await.unwrap();
        let idx = create_vm(&state).await.unwrap();
        assert_eq!(
            idx, 0,
            "phải nhận diện index tái dùng (0), không phải max (1)"
        );
    }

    #[tokio::test]
    async fn launch_khong_fingerprint_van_cho_boot() {
        // Tài khoản CHƯA có fingerprint (hw = None) vẫn phải chờ boot trước khi
        // chạy adb (restore/start_app) — nếu không sẽ đua với quá trình boot.
        let (state, _memuc, adb) = make_state("nohw");
        let restored = launch_instance(&state, 0, "acc-khong-hw").await.unwrap();
        assert!(!restored, "chưa có snapshot → không restore");
        assert_eq!(
            adb.boot_wait_count(),
            1,
            "phải chờ boot dù không có fingerprint"
        );
    }

    #[tokio::test]
    async fn provision_ap_hardware_va_android_id() {
        let (state, memuc, adb) = make_state("hw");
        let idx = provision(&state, "acc1", &hw()).await.unwrap();

        // Config phần cứng được áp qua memuc.
        assert_eq!(
            memuc.config_value(idx, "imei").as_deref(),
            Some("860504493831119")
        );
        assert_eq!(
            memuc.config_value(idx, "microvirt_vm_model").as_deref(),
            Some("FRD-L19")
        );
        assert_eq!(
            memuc.config_value(idx, "custom_resolution").as_deref(),
            Some("1080 1920 320")
        );
        assert_eq!(memuc.config_value(idx, "enable_su").as_deref(), Some("1"));
        // android_id áp qua adb.
        assert_eq!(adb.android_id_of(idx).as_deref(), Some("a1b2c3d4e5f6"));
    }

    #[tokio::test]
    async fn vong_doi_provision_teardown_provision_giu_du_lieu() {
        let (state, _memuc, adb) = make_state("cycle");

        // Phiên 1: cấp phát, ghi dữ liệu, kết thúc (backup + hủy).
        let idx1 = provision(&state, "acc1", &hw()).await.unwrap();
        adb.set_device_data(idx1, b"session-A".to_vec());
        teardown(&state, idx1, "acc1").await.unwrap();

        // VM đã bị hủy.
        let list = state.memuc.list_instances().await.unwrap();
        assert!(list.iter().all(|v| v.index != idx1), "VM phải bị hủy");

        // Xóa sạch dữ liệu thiết bị mô phỏng → mọi dữ liệu ở phiên 2 CHỈ có thể
        // đến từ restore (chứng minh restore thật sự chạy, không phải state cũ).
        adb.clear_devices();

        // Phiên 2: cấp phát lại → phải restore dữ liệu phiên 1 từ kho.
        let idx2 = provision(&state, "acc1", &hw()).await.unwrap();
        assert_eq!(adb.device_data(idx2).as_deref(), Some(&b"session-A"[..]));
    }

    #[tokio::test]
    async fn swap_flash_sach_ap_fingerprint_va_restore() {
        let (state, memuc, adb) = make_state("swap");

        // Chuẩn bị snapshot cho tài khoản mới.
        adb.set_device_data(0, b"session-new".to_vec());
        backup_and_record(&state, 0, "acc_new").await.unwrap();
        // VM đang chạy tài khoản CŨ (dữ liệu khác).
        adb.set_device_data(0, b"session-OLD".to_vec());

        swap_account(&state, 0, "acc_new", &hw()).await.unwrap();

        // Fingerprint riêng đã áp (memuc + android_id).
        assert_eq!(
            memuc.config_value(0, "imei").as_deref(),
            Some("860504493831119")
        );
        assert_eq!(adb.android_id_of(0).as_deref(), Some("a1b2c3d4e5f6"));
        // Dữ liệu account cũ đã bị flash sạch & thay bằng session account mới.
        assert_eq!(adb.device_data(0).as_deref(), Some(&b"session-new"[..]));
    }

    #[tokio::test]
    async fn launch_nap_lai_fingerprint_da_luu() {
        let (state, memuc, adb) = make_state("launch");
        // Lưu fingerprint cho VM 0 (như khi tạo tài khoản).
        state.set_hardware(0, hw()).await;

        // Khởi chạy → phải NẠP LẠI fingerprint đã lưu & áp.
        launch_instance(&state, 0, "acc1").await.unwrap();

        assert_eq!(
            memuc.config_value(0, "imei").as_deref(),
            Some("860504493831119")
        );
        assert_eq!(
            memuc.config_value(0, "microvirt_vm_model").as_deref(),
            Some("FRD-L19")
        );
        assert_eq!(adb.android_id_of(0).as_deref(), Some("a1b2c3d4e5f6"));
    }

    #[tokio::test]
    async fn clone_from_base_ap_fingerprint_rieng_va_luu_db() {
        let (state, memuc, adb) = make_state("clone");
        // Base = VM 0 (seeded). Clone → VM mới.
        let new_idx = clone_from_base(&state, 0, "acc_clone", &hw())
            .await
            .unwrap();
        assert!(new_idx >= 2, "clone tạo VM index mới");

        // Fingerprint riêng đã áp cho clone + lưu DB (nạp lại được).
        assert_eq!(
            memuc.config_value(new_idx, "imei").as_deref(),
            Some("860504493831119")
        );
        assert_eq!(adb.android_id_of(new_idx).as_deref(), Some("a1b2c3d4e5f6"));
        assert_eq!(
            state.hardware_of(new_idx).await.unwrap().model,
            "FRD-L19",
            "fingerprint đã lưu DB cho VM clone"
        );

        // VM clone tồn tại.
        let list = state.memuc.list_instances().await.unwrap();
        assert!(list.iter().any(|v| v.index == new_idx));
    }

    #[tokio::test]
    async fn warm_pool_refill_va_acquire() {
        let (state, memuc, _adb) = make_state("pool");

        // Nạp pool tới 2 VM nóng (clone từ base 0).
        let n = pool_refill(&state, 0, 2).await.unwrap();
        assert_eq!(n, 2, "pool có 2 VM nóng");

        // Lấy 1 VM từ pool gán account → áp fingerprint riêng.
        let idx = pool_acquire(&state, 0, "accP", &hw()).await.unwrap();
        assert_eq!(
            memuc.config_value(idx, "imei").as_deref(),
            Some("860504493831119")
        );
        // Pool giảm còn 1 (lấy tức thì, không cold-boot).
        assert_eq!(pool_size(&state).await, 1);
    }

    #[tokio::test]
    async fn cong_quoc_gia_bo_qua_khi_khong_yeu_cau() {
        // Không đặt quốc gia yêu cầu → không chặn (kể cả geo trả None).
        let (state, _m, _a) = make_state_geo("cc_skip", Arc::new(FixedGeo(None)));
        assert!(assert_country_match(&state, 0).await.is_ok());
    }

    #[tokio::test]
    async fn cong_quoc_gia_cho_qua_khi_khop() {
        let (state, _m, _a) = make_state_geo("cc_ok", Arc::new(FixedGeo(Some("VN"))));
        state.set_country(0, Some("vn".into())).await; // so khớp không phân biệt hoa/thường
        assert!(assert_country_match(&state, 0).await.is_ok());
    }

    #[tokio::test]
    async fn cong_quoc_gia_chan_khi_lech() {
        let (state, _m, _a) = make_state_geo("cc_bad", Arc::new(FixedGeo(Some("VN"))));
        state.set_country(0, Some("US".into())).await;
        match assert_country_match(&state, 0).await {
            Err(AppError::CountryMismatch { actual, expected }) => {
                assert_eq!(actual, "VN");
                assert_eq!(expected, "US");
            }
            other => panic!("phải chặn vì lệch quốc gia, nhận: {other:?}"),
        }
    }

    #[tokio::test]
    async fn cong_quoc_gia_chan_khi_khong_xac_thuc_duoc() {
        // Có yêu cầu quốc gia nhưng geo không tra được → chặn (an toàn).
        let (state, _m, _a) = make_state_geo("cc_unv", Arc::new(FixedGeo(None)));
        state.set_country(0, Some("VN".into())).await;
        assert!(matches!(
            assert_country_match(&state, 0).await,
            Err(AppError::CountryUnverified(_))
        ));
    }
}
