//! Orchestrator "Môi trường dùng một lần" (§3 thiết kế Backup/Restore).
//! Điều phối vòng đời: provision (tạo VM sạch → áp hardware → start → restore)
//! và teardown (backup → đồng bộ → HỦY VM). Bất biến an toàn: chỉ hủy VM sau khi
//! backup + ghi CSDL thành công (R-15).

use std::collections::HashSet;
use std::fs;

use crate::error::{AppError, AppResult};
use crate::model::{HardwareProfile, SnapshotMeta, SnapshotRecord, DEFAULT_BLOAT, TIKTOK_PKG};
use crate::state::{now_ms, SharedState};

/// Tập index VM hiện có (từ emulator).
async fn index_set(state: &SharedState) -> AppResult<HashSet<u32>> {
    Ok(state
        .emulator
        .list_instances()
        .await?
        .into_iter()
        .map(|i| i.index)
        .collect())
}

/// Nhận diện index VM **mới** = index xuất hiện sau khi tạo mà trước đó chưa có.
/// Đáng tin hơn `max(index)` vì emulator có thể **tái dùng** index đã xóa (lấp khoảng
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
    state.queue.run(state.emulator.create()).await?;
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

/// Áp toàn bộ cấu hình phần cứng (fingerprint) vào VM: các khoá MuMu `simulation` +
/// **độ phân giải/DPI** (cửa sổ VM mặc định khớp thiết bị fake). android_id áp riêng (adb).
pub async fn apply_hw_config(
    state: &SharedState,
    index: u32,
    hw: &HardwareProfile,
) -> AppResult<()> {
    for (key, value) in hw.emulator_pairs() {
        state.emulator.set_config(index, key, &value).await?;
    }
    state
        .emulator
        .set_resolution(index, hw.res_width, hw.res_height, hw.dpi)
        .await?;
    Ok(())
}

/// Re-apply runtime identity after Android boot and after install/restore mutations.
async fn reassert_runtime_fingerprint(
    state: &SharedState,
    index: u32,
    hw: &HardwareProfile,
    require_android_id: bool,
) -> AppResult<()> {
    match state
        .adb
        .apply_display_profile(index, hw.res_width, hw.res_height, hw.dpi)
        .await
    {
        Ok(true) => {}
        Ok(false) => tracing::warn!(
            index,
            width = hw.res_width,
            height = hw.res_height,
            dpi = hw.dpi,
            "Khong verify duoc wm size/density theo profile"
        ),
        Err(e) => tracing::warn!(
            index,
            error = %e,
            "Khong apply duoc wm size/density runtime"
        ),
    }

    if require_android_id {
        state.adb.apply_android_id(index, &hw.android_id).await?;
    } else if let Err(e) = state.adb.apply_android_id(index, &hw.android_id).await {
        tracing::warn!(
            index,
            error = %e,
            "Khong re-apply duoc android_id runtime"
        );
    }

    if let Err(e) = state.adb.lock_device_identity(index, hw).await {
        tracing::warn!(
            index,
            error = %e,
            "Khong re-apply duoc ro.product/build fingerprint"
        );
    }

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
            let _ = state.emulator.stop(index).await;
            let _ = state.emulator.remove(index).await;
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
    state.queue.run(state.emulator.start(index)).await?;
    state.mark_launched(index).await;
    state.adb.wait_boot_completed(index).await?;
    // Đẩy binary magisk (resetprop) vào VM nếu người dùng đã cấu hình Magisk APK —
    // VM disposable nên đẩy mỗi lần provision. Sau đó khóa model post-boot (chống MuMu
    // random ro.product.model). Best-effort — no-op nếu chưa có Magisk APK.
    if let Some(bin) = state.magisk_bin() {
        let _ = state
            .adb
            .push_resetprop(index, &bin.to_string_lossy())
            .await;
    }
    reassert_runtime_fingerprint(state, index, hw, true).await?;
    auto_debloat(state, index).await;

    let mut needs_final_reassert = false;

    // Cài TikTok TRƯỚC restore (pkg phải tồn tại để restore ghi /data/data/<pkg>).
    // Idempotent (`install_apk -r`). Bắt buộc thành công cho luồng profile.
    if let Some(apk) = apk_path {
        state.adb.install_apk(index, apk).await?;
        needs_final_reassert = true;
    }

    // Restore snapshot mới nhất nếu có (verify sha256 trước).
    if let Some(db) = &state.db {
        if let Some(rec) = db.latest_snapshot(account_key)? {
            if !state.store.verify(&rec.storage_key, &rec.sha256).await? {
                return Err(AppError::CommandFailed(format!(
                    "Snapshot mới nhất của {account_key} hỏng hoặc sai checksum — không chạy VM để tránh mất session"
                )));
            }

            if let Some(expected) = rec
                .apk_version
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty() && *v != "unknown")
            {
                let actual = state.adb.apk_version(index, TIKTOK_PKG).await?;
                if actual.trim() != expected {
                    return Err(AppError::CommandFailed(format!(
                        "APK TikTok lệch phiên bản: snapshot={expected}, vm={actual}. Từ chối restore để tránh hỏng dữ liệu app"
                    )));
                }
            }

            let tmp = std::env::temp_dir().join(format!("mpm-prov-{index}-{}.tar.zst", now_ms()));
            // Dọn tmp MỌI nhánh (get/restore lỗi cũng không rò file tạm).
            let res = async {
                state.store.get(&rec.storage_key, &tmp).await?;
                state.adb.restore(index, TIKTOK_PKG, &tmp).await
            }
            .await;
            let _ = fs::remove_file(&tmp);
            res?;
            needs_final_reassert = true;
        }
    }

    if needs_final_reassert {
        reassert_runtime_fingerprint(state, index, hw, false).await?;
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
    let _ = state.emulator.stop(index).await;
    if let Err(e) = state.emulator.remove(index).await {
        tracing::warn!(index, error = %e, "remove VM lỗi — thử lại sau khi stop");
        let _ = state.emulator.stop(index).await;
        state.emulator.remove(index).await?;
    }
    state.forget(index).await;

    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::MockClient;
    use crate::geo::MockGeolocator;
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

    fn make_state(tag: &str) -> (SharedState, Arc<MockClient>, Arc<MockAdbWorker>) {
        make_state_geo(tag, Arc::new(MockGeolocator))
    }

    fn make_state_geo(
        tag: &str,
        geo: Arc<dyn crate::geo::IpGeolocator>,
    ) -> (SharedState, Arc<MockClient>, Arc<MockAdbWorker>) {
        let emulator = Arc::new(MockClient::new());
        let adb = Arc::new(MockAdbWorker::new());
        let dir = std::env::temp_dir().join(format!("mpm_orch_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
        let db = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
        let state: SharedState = Arc::new(AppState::new(
            emulator.clone(),
            geo,
            adb.clone(),
            store,
            AppSettings::default(),
            Some(db),
            HashMap::new(),
        ));
        (state, emulator, adb)
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
        let (state, emulator, adb) = make_state("hw");
        let idx = provision(&state, "acc1", &hw(), None).await.unwrap();

        // Config phần cứng đi qua EmulatorClient; android_id áp riêng qua adb.
        assert_eq!(
            emulator.config_value(idx, "imei").await.as_deref(),
            Some("860504493831119")
        );
        assert_eq!(
            emulator.config_value(idx, "model").await.as_deref(),
            Some("FRD-L19")
        );
        assert_eq!(
            emulator.config_value(idx, "manufacturer").await.as_deref(),
            Some("HUAWEI")
        );
        assert_eq!(
            emulator.config_value(idx, "brand").await.as_deref(),
            Some("HUAWEI")
        );
        assert_eq!(
            emulator.config_value(idx, "mac_address").await.as_deref(),
            Some("02:00:00:11:22:33")
        );
        assert_eq!(adb.android_id_of(idx).as_deref(), Some("a1b2c3d4e5f6"));
        assert_eq!(adb.display_profile_of(idx), Some((1080, 1920, 320)));
        assert_eq!(
            adb.lock_count_of(idx),
            1,
            "provision khong cai/restore chi can khoa fingerprint mot lan sau boot"
        );
    }

    #[tokio::test]
    async fn provision_reassert_fingerprint_sau_cai_apk() {
        let (state, _emulator, adb) = make_state("reassert_install");
        let idx = provision(&state, "acc_install", &hw(), Some("TikTok.apk"))
            .await
            .unwrap();

        assert_eq!(adb.display_profile_of(idx), Some((1080, 1920, 320)));
        assert_eq!(
            adb.android_id_of(idx).as_deref(),
            Some("a1b2c3d4e5f6"),
            "android_id duoc re-apply sau install"
        );
        assert_eq!(
            adb.lock_count_of(idx),
            2,
            "provision phai re-assert resetprop sau install de chong MuMu/GMS ghi de muon"
        );
    }

    #[tokio::test]
    async fn vong_doi_provision_teardown_provision_giu_du_lieu() {
        let (state, _emulator, adb) = make_state("cycle");

        // Phiên 1: cấp phát, ghi dữ liệu, kết thúc (backup + hủy).
        let idx1 = provision(&state, "acc1", &hw(), None).await.unwrap();
        adb.set_device_data(idx1, b"session-A".to_vec());
        teardown(&state, idx1, "acc1").await.unwrap();

        // VM đã bị hủy.
        let list = state.emulator.list_instances().await.unwrap();
        assert!(list.iter().all(|v| v.index != idx1), "VM phải bị hủy");

        // Xóa sạch dữ liệu thiết bị mô phỏng → mọi dữ liệu ở phiên 2 CHỈ có thể
        // đến từ restore (chứng minh restore thật sự chạy, không phải state cũ).
        adb.clear_devices();

        // Phiên 2: cấp phát lại → phải restore dữ liệu phiên 1 từ kho.
        let idx2 = provision(&state, "acc1", &hw(), None).await.unwrap();
        assert_eq!(adb.device_data(idx2).as_deref(), Some(&b"session-A"[..]));
    }

    #[tokio::test]
    async fn provision_tu_choi_snapshot_sai_checksum() {
        let (state, _m, _adb) = make_state("bad_sha");
        let before: std::collections::HashSet<u32> = state
            .emulator
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();

        let raw = std::env::temp_dir().join(format!("mpm-bad-sha-{}.tar", std::process::id()));
        fs::write(&raw, b"session").unwrap();
        let stored = state.store.put("acc_bad/1.tar.zst", &raw).await.unwrap();
        let _ = fs::remove_file(&raw);
        let db = state.db.as_ref().unwrap();
        db.record_snapshot(
            "acc_bad",
            "acc_bad/1.tar.zst",
            &SnapshotMeta {
                sha256: "0".repeat(64),
                size_bytes: stored.size_bytes,
                apk_version: "mock-1.0".into(),
            },
            now_ms(),
        )
        .unwrap();

        let err = provision(&state, "acc_bad", &hw(), None).await.unwrap_err();
        assert!(
            err.to_string().contains("sai checksum"),
            "lỗi phải nói rõ checksum: {err}"
        );
        let after: std::collections::HashSet<u32> = state
            .emulator
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();
        assert_eq!(after, before, "provision lỗi phải dọn VM vừa tạo");
    }

    #[tokio::test]
    async fn provision_tu_choi_restore_khi_apk_version_lech() {
        let (state, _m, _adb) = make_state("apk_mismatch");
        let before: std::collections::HashSet<u32> = state
            .emulator
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();

        let raw = std::env::temp_dir().join(format!("mpm-apk-mismatch-{}.tar", std::process::id()));
        fs::write(&raw, b"session").unwrap();
        let stored = state.store.put("acc_apk/1.tar.zst", &raw).await.unwrap();
        let _ = fs::remove_file(&raw);
        let db = state.db.as_ref().unwrap();
        db.record_snapshot(
            "acc_apk",
            "acc_apk/1.tar.zst",
            &SnapshotMeta {
                sha256: stored.sha256,
                size_bytes: stored.size_bytes,
                apk_version: "mock-2.0".into(),
            },
            now_ms(),
        )
        .unwrap();

        let err = provision(&state, "acc_apk", &hw(), None).await.unwrap_err();
        assert!(
            err.to_string().contains("APK TikTok lệch phiên bản"),
            "lỗi phải chặn version mismatch: {err}"
        );
        let after: std::collections::HashSet<u32> = state
            .emulator
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();
        assert_eq!(after, before, "provision lỗi phải dọn VM vừa tạo");
    }

    #[tokio::test]
    async fn provision_don_dep_vm_khi_buoc_sau_create_loi() {
        // NGUYÊN TỬ: provision cài APK (Some path bịa) — MockAdb.install_apk ok, nên
        // đường lỗi ở đây khó kích. Thay vào đó kiểm luồng thành công không rò VM:
        // provision → teardown → tập index trở lại như trước.
        let (state, _m, _adb) = make_state("atomic");
        let before: std::collections::HashSet<u32> = state
            .emulator
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
            .emulator
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();
        assert_eq!(after, before, "sau teardown, tập index trở lại như trước");
    }
}
