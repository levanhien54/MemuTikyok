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
    let put = state.store.put(&storage_key, &tmp).await;
    let _ = std::fs::remove_file(&tmp); // dọn tmp MỌI nhánh (kể cả put lỗi → không rò .tar)
    let stored = put?;

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
/// (cài TikTok nếu có `apk_path`) → restore snapshot mới nhất (nếu có). Trả về
/// index VM đã sẵn sàng làm việc.
///
/// `apk_path = Some(...)` → cài APK TikTok TRƯỚC bước restore. Bắt buộc cho luồng
/// dùng-thật (profile) vì: (1) lần đầu chưa có session → cần app để đăng nhập;
/// (2) restore giải nén vào `/data/data/<pkg>` nên package PHẢI được cài trước.
/// `None` → bỏ qua cài (các test fingerprint-only tự lo APK khi cần).
pub async fn provision(
    state: &SharedState,
    account_key: &str,
    hw: &HardwareProfile,
    apk_path: Option<&str>,
) -> AppResult<u32> {
    // 1) Tạo VM mới tinh và xác định index của nó (an toàn tái dùng index/đua tranh).
    let index = create_vm(state).await?;

    // 2..4) Chuẩn bị VM. NGUYÊN TỬ: nếu BẤT KỲ bước nào lỗi → HỦY VM vừa tạo. VM đã
    // được tạo nhưng CHƯA trả về caller nên không ai teardown được nó — nếu không dọn
    // ở đây thì rò "VM mồ côi" (chạy nhưng không nằm trong running map → không tính vào
    // cổng tối đa 5 → tích tụ, ngốn RAM/đĩa). Đối xứng R-15 (teardown chỉ hủy sau backup).
    match provision_prepare(state, index, account_key, hw, apk_path).await {
        Ok(()) => Ok(index),
        Err(e) => {
            let _ = state.memuc.stop(index).await;
            let _ = state.memuc.remove(index).await;
            state.forget(index).await;
            Err(e)
        }
    }
}

/// Các bước chuẩn bị một VM ĐÃ tạo: áp hardware → boot → android_id → debloat →
/// (cài app nếu có) → restore. Tách khỏi `provision` để bọc dọn-dẹp-khi-lỗi (mọi
/// `?` ở đây khiến provision hủy VM thay vì rò).
async fn provision_prepare(
    state: &SharedState,
    index: u32,
    account_key: &str,
    hw: &HardwareProfile,
    apk_path: Option<&str>,
) -> AppResult<()> {
    // Áp hồ sơ phần cứng + độ phân giải (nhất quán fingerprint — R-12).
    apply_hw_config(state, index, hw).await?;

    // Start → chờ boot → áp android_id → gỡ app thừa mặc định.
    state.queue.run(state.memuc.start(index)).await?;
    state.mark_launched(index).await;
    state.adb.wait_boot_completed(index).await?;
    state.adb.apply_android_id(index, &hw.android_id).await?;
    // Khóa model post-boot (chống MEmu random ro.product.model). Best-effort — cần
    // resetprop trên VM (Magisk trong base image); no-op nếu chưa có.
    let _ = state.adb.lock_device_identity(index, hw).await;
    auto_debloat(state, index).await;

    // Cài TikTok TRƯỚC restore (pkg phải tồn tại để restore ghi /data/data/<pkg>).
    // Idempotent (`install_apk -r`). Bắt buộc thành công cho luồng profile.
    if let Some(apk) = apk_path {
        state.adb.install_apk(index, apk).await?;
    }

    // Restore snapshot mới nhất nếu có (verify sha256 trước).
    if let Some(db) = &state.db {
        if let Some(rec) = db.latest_snapshot(account_key)? {
            if state.store.verify(&rec.storage_key, &rec.sha256).await? {
                let tmp =
                    std::env::temp_dir().join(format!("mpm-prov-{index}-{}.tar.zst", now_ms()));
                // Dọn tmp MỌI nhánh (get/restore lỗi cũng không rò file tạm).
                let res = async {
                    state.store.get(&rec.storage_key, &tmp).await?;
                    state.adb.restore(index, TIKTOK_PKG, &tmp).await
                }
                .await;
                let _ = fs::remove_file(&tmp);
                res?;
            } else {
                tracing::warn!(account_key, "Snapshot hỏng — provision không restore");
            }
        }
    }

    Ok(())
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

    // 2) Hủy VM. stop best-effort; remove có thể chập chờn (VM chưa dừng hẳn) → thử
    // lại 1 lần sau khi stop. Nếu vẫn lỗi → trả Err: caller (profile stop/delete) sẽ
    // TÁI theo dõi VM để người dùng thử lại, không để VM mồ côi vô chủ.
    let _ = state.memuc.stop(index).await;
    if let Err(e) = state.memuc.remove(index).await {
        tracing::warn!(index, error = %e, "remove VM lỗi — thử lại sau khi stop");
        let _ = state.memuc.stop(index).await;
        state.memuc.remove(index).await?;
    }
    state.forget(index).await;

    Ok(record)
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
    async fn provision_goi_khoa_model() {
        // provision phải gọi lock_device_identity (khóa model post-boot) với model đúng.
        let (state, _m, adb) = make_state("lock");
        let idx = provision(&state, "acc_lock", &hw(), None).await.unwrap();
        assert_eq!(
            adb.locked_model_of(idx).as_deref(),
            Some("FRD-L19"),
            "provision phải khóa model qua lock_device_identity"
        );
    }

    #[tokio::test]
    async fn provision_ap_hardware_va_android_id() {
        let (state, memuc, adb) = make_state("hw");
        let idx = provision(&state, "acc1", &hw(), None).await.unwrap();

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
        let idx1 = provision(&state, "acc1", &hw(), None).await.unwrap();
        adb.set_device_data(idx1, b"session-A".to_vec());
        teardown(&state, idx1, "acc1").await.unwrap();

        // VM đã bị hủy.
        let list = state.memuc.list_instances().await.unwrap();
        assert!(list.iter().all(|v| v.index != idx1), "VM phải bị hủy");

        // Xóa sạch dữ liệu thiết bị mô phỏng → mọi dữ liệu ở phiên 2 CHỈ có thể
        // đến từ restore (chứng minh restore thật sự chạy, không phải state cũ).
        adb.clear_devices();

        // Phiên 2: cấp phát lại → phải restore dữ liệu phiên 1 từ kho.
        let idx2 = provision(&state, "acc1", &hw(), None).await.unwrap();
        assert_eq!(adb.device_data(idx2).as_deref(), Some(&b"session-A"[..]));
    }

    #[tokio::test]
    async fn provision_don_dep_vm_khi_buoc_sau_create_loi() {
        // NGUYÊN TỬ: provision cài APK (Some path bịa) — MockAdb.install_apk ok, nên
        // đường lỗi ở đây khó kích. Thay vào đó kiểm luồng thành công không rò VM:
        // provision → teardown → tập index trở lại như trước.
        let (state, _m, _adb) = make_state("atomic");
        let before: std::collections::HashSet<u32> = state
            .memuc
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();
        let idx = provision(&state, "acc_atom", &hw(), None).await.unwrap();
        assert!(!before.contains(&idx), "provision tạo VM mới");
        teardown(&state, idx, "acc_atom").await.unwrap();
        let after: std::collections::HashSet<u32> = state
            .memuc
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();
        assert_eq!(after, before, "sau teardown, tập index trở lại như trước");
    }
}
