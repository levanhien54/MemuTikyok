//! Tauri commands — biên giới IPC được UI gọi qua `invoke` (§8.4 SRS).
//! Lệnh thao tác đi qua Command Queue để kiểm soát tải (§8.3).

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::model::{
    AccountProfile, AppSettings, BulkOperation, CreateInstancePayload, EmulatorTell,
    HardwareProfile, Instance, SnapshotRecord, TIKTOK_PKG,
};
use crate::orchestrator;
use crate::state::{now_ms, SharedState};

#[tauri::command]
pub async fn list_instances(state: State<'_, SharedState>) -> AppResult<Vec<Instance>> {
    let list = state.memuc.list_instances().await?;
    Ok(state.merge_metadata(list).await)
}

#[tauri::command]
pub async fn start_instance(index: u32, state: State<'_, SharedState>) -> AppResult<()> {
    state.queue.run(state.memuc.start(index)).await?;
    state.mark_launched(index).await;
    Ok(())
}

#[tauri::command]
pub async fn stop_instance(index: u32, state: State<'_, SharedState>) -> AppResult<()> {
    state.queue.run(state.memuc.stop(index)).await
}

#[tauri::command]
pub async fn reboot_instance(index: u32, state: State<'_, SharedState>) -> AppResult<()> {
    state.queue.run(state.memuc.reboot(index)).await?;
    state.mark_launched(index).await;
    Ok(())
}

/// Tạo VM rồi gán hồ sơ tài khoản cho VM mới (index lớn nhất sau khi list lại).
#[tauri::command]
pub async fn create_instance(
    payload: CreateInstancePayload,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    state.queue.run(state.memuc.create()).await?;
    let list = state.memuc.list_instances().await?;
    if let Some(new_index) = list.iter().map(|i| i.index).max() {
        state.set_account(new_index, payload.account).await;
        if !payload.note.trim().is_empty() {
            state.set_note(new_index, payload.note).await;
        }
        // Quốc gia yêu cầu (đối chiếu khi khởi chạy). Rỗng → không ràng buộc.
        if payload
            .country
            .as_deref()
            .is_some_and(|c| !c.trim().is_empty())
        {
            state.set_country(new_index, payload.country).await;
        }
        // Sinh & lưu fingerprint gắn với tài khoản (áp lại mỗi lần khởi chạy).
        if let Ok(hw) = crate::fingerprint::generate() {
            state.set_hardware(new_index, hw.clone()).await;
            // Áp cấu hình + độ phân giải NGAY → cửa sổ VM mặc định khớp thiết bị fake.
            let _ = orchestrator::apply_hw_config(state.inner(), new_index, &hw).await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_account(
    index: u32,
    account: AccountProfile,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    state.set_account(index, account).await;
    Ok(())
}

#[tauri::command]
pub async fn update_note(index: u32, note: String, state: State<'_, SharedState>) -> AppResult<()> {
    state.set_note(index, note).await;
    Ok(())
}

/// Cập nhật quốc gia yêu cầu của VM (gate khi khởi chạy). None/rỗng = bỏ ràng buộc.
#[tauri::command]
pub async fn update_country(
    index: u32,
    country: Option<String>,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    state.set_country(index, country).await;
    Ok(())
}

#[tauri::command]
pub async fn remove_instance(index: u32, state: State<'_, SharedState>) -> AppResult<()> {
    state.memuc.remove(index).await?;
    state.forget(index).await;
    Ok(())
}

#[tauri::command]
pub async fn rename_instance(
    index: u32,
    title: String,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    state.memuc.rename(index, &title).await
}

/// Thao tác hàng loạt: mỗi VM chạy qua queue (giới hạn song song). Một VM lỗi
/// không dừng cả lô — thu thập rồi báo lỗi tổng hợp (§10.3 partial failure).
#[tauri::command]
pub async fn bulk_action(
    operation: BulkOperation,
    indexes: Vec<u32>,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    let mut errors = Vec::new();
    for index in indexes {
        // Cổng quốc gia chỉ áp cho Start (khởi chạy) — nhất quán với nút Play.
        if matches!(operation, BulkOperation::Start) {
            if let Err(e) = orchestrator::assert_country_match(state.inner(), index).await {
                errors.push(format!("VM {index}: {e}"));
                continue;
            }
        }
        let memuc = state.memuc.clone();
        let result = state
            .queue
            .run(async move {
                match operation {
                    BulkOperation::Start => memuc.start(index).await,
                    BulkOperation::Stop => memuc.stop(index).await,
                    BulkOperation::Reboot => memuc.reboot(index).await,
                }
            })
            .await;
        match result {
            Ok(()) => {
                if matches!(operation, BulkOperation::Start | BulkOperation::Reboot) {
                    state.mark_launched(index).await;
                }
            }
            Err(e) => errors.push(format!("VM {index}: {e}")),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(crate::error::AppError::CommandFailed(errors.join("; ")))
    }
}

/// Backup dữ liệu phiên TikTok của VM ra kho (Cloud/local) + ghi CSDL.
/// `account_key` định danh tài khoản (ổn định qua các phiên, vd username).
#[tauri::command]
pub async fn backup_instance(
    index: u32,
    account_key: String,
    state: State<'_, SharedState>,
) -> AppResult<SnapshotRecord> {
    orchestrator::backup_and_record(state.inner(), index, &account_key).await
}

/// Nạp snapshot mới nhất của `account_key` vào VM (có verify sha256 trước khi restore).
#[tauri::command]
pub async fn restore_instance(
    index: u32,
    account_key: String,
    state: State<'_, SharedState>,
) -> AppResult<SnapshotRecord> {
    let db = state
        .db
        .as_ref()
        .ok_or_else(|| AppError::CommandFailed("Không có cơ sở dữ liệu snapshot".into()))?;
    let rec = db
        .latest_snapshot(&account_key)?
        .ok_or_else(|| AppError::InvalidInput("Chưa có snapshot cho tài khoản này".into()))?;

    // Toàn vẹn: từ chối restore nếu archive hỏng (§7 thiết kế).
    if !state.store.verify(&rec.storage_key, &rec.sha256).await? {
        return Err(AppError::CommandFailed(
            "Snapshot hỏng: sha256 không khớp".into(),
        ));
    }

    let tmp = std::env::temp_dir().join(format!("mpm-restore-{index}-{}.tar.zst", now_ms()));
    state.store.get(&rec.storage_key, &tmp).await?;
    state.adb.restore(index, TIKTOK_PKG, &tmp).await?;
    let _ = std::fs::remove_file(&tmp);

    Ok(rec)
}

/// Cấp phát một VM dùng-một-lần cho tài khoản: tạo sạch → áp hardware → start →
/// restore snapshot mới nhất. Trả về index VM đã sẵn sàng.
#[tauri::command]
pub async fn provision_session(
    account_key: String,
    hardware: HardwareProfile,
    state: State<'_, SharedState>,
) -> AppResult<u32> {
    orchestrator::provision(state.inner(), &account_key, &hardware).await
}

/// Clone từ base image + áp fingerprint riêng cho tài khoản → chạy. Trả index VM mới.
#[tauri::command]
pub async fn clone_from_base(
    base_index: u32,
    account_key: String,
    hardware: HardwareProfile,
    state: State<'_, SharedState>,
) -> AppResult<u32> {
    orchestrator::clone_from_base(state.inner(), base_index, &account_key, &hardware).await
}

/// Nạp warm pool tới `target` VM nóng (clone base + boot sẵn). Trả số VM trong pool.
#[tauri::command]
pub async fn warm_pool_refill(
    base_index: u32,
    target: usize,
    state: State<'_, SharedState>,
) -> AppResult<usize> {
    orchestrator::pool_refill(state.inner(), base_index, target).await
}

/// Lấy 1 VM nóng từ pool gán cho tài khoản (áp fingerprint riêng). Trả index.
#[tauri::command]
pub async fn warm_pool_acquire(
    base_index: u32,
    account_key: String,
    hardware: HardwareProfile,
    state: State<'_, SharedState>,
) -> AppResult<u32> {
    orchestrator::pool_acquire(state.inner(), base_index, &account_key, &hardware).await
}

#[tauri::command]
pub async fn warm_pool_size(state: State<'_, SharedState>) -> AppResult<usize> {
    Ok(orchestrator::pool_size(state.inner()).await)
}

/// Khởi chạy VM: nạp lại fingerprint đã lưu (DB) & áp → start → restore session.
#[tauri::command]
pub async fn launch_instance(
    index: u32,
    account_key: String,
    state: State<'_, SharedState>,
) -> AppResult<bool> {
    orchestrator::launch_instance(state.inner(), index, &account_key).await
}

/// Cài APK (vd TikTok) vào VM.
#[tauri::command]
pub async fn install_apk(
    index: u32,
    apk_path: String,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    state.adb.install_apk(index, &apk_path).await
}

/// Cài TikTok dùng đường dẫn trong Settings (fallback mặc định).
#[tauri::command]
pub async fn install_tiktok(index: u32, state: State<'_, SharedState>) -> AppResult<()> {
    let path = {
        state
            .settings
            .lock()
            .await
            .tiktok_apk_path
            .clone()
            .filter(|p| !p.trim().is_empty())
    }
    .unwrap_or_else(|| crate::model::DEFAULT_TIKTOK_APK.to_string());
    state.adb.install_apk(index, &path).await
}

/// Gỡ/vô hiệu hóa danh sách app (bloat, app không dùng) khỏi VM.
#[tauri::command]
pub async fn disable_apps(
    index: u32,
    packages: Vec<String>,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    for pkg in packages {
        state.adb.disable_app(index, &pkg).await?;
    }
    Ok(())
}

/// Liệt kê app bên thứ 3 trong VM (để chọn gỡ).
#[tauri::command]
pub async fn list_apps(index: u32, state: State<'_, SharedState>) -> AppResult<Vec<String>> {
    state.adb.list_third_party_apps(index).await
}

/// Scan dấu vết emulator của một VM (chẩn đoán chống phát hiện).
#[tauri::command]
pub async fn scan_emulator(
    index: u32,
    state: State<'_, SharedState>,
) -> AppResult<Vec<EmulatorTell>> {
    state.adb.scan_emulator_tells(index).await
}

/// Ẩn/sửa các dấu vết sửa được (best-effort).
#[tauri::command]
pub async fn harden_vm(index: u32, state: State<'_, SharedState>) -> AppResult<()> {
    state.adb.harden(index).await
}

/// Lấy fingerprint (đã lưu DB) của một VM để hiển thị.
#[tauri::command]
pub async fn get_hardware(
    index: u32,
    state: State<'_, SharedState>,
) -> AppResult<Option<HardwareProfile>> {
    Ok(state.hardware_of(index).await)
}

/// Đổi tài khoản trên VM đang chạy (nhanh: flash sạch + fingerprint + reboot + restore).
#[tauri::command]
pub async fn swap_account(
    index: u32,
    account_key: String,
    hardware: HardwareProfile,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    orchestrator::swap_account(state.inner(), index, &account_key, &hardware).await
}

/// Kết thúc phiên: backup dữ liệu về kho/CSDL rồi HỦY VM (disposable).
#[tauri::command]
pub async fn teardown_session(
    index: u32,
    account_key: String,
    state: State<'_, SharedState>,
) -> AppResult<SnapshotRecord> {
    orchestrator::teardown(state.inner(), index, &account_key).await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, SharedState>) -> AppResult<AppSettings> {
    Ok(state.settings.lock().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    mut settings: AppSettings,
    state: State<'_, SharedState>,
) -> AppResult<AppSettings> {
    // Ràng buộc giá trị an toàn: poll ≥ 250ms (interval(0) panic), concurrency ≥ 1.
    settings.poll_interval_ms = settings.poll_interval_ms.max(250);
    settings.max_concurrency = settings.max_concurrency.max(1);
    {
        let mut guard = state.settings.lock().await;
        *guard = settings.clone();
    }
    // Lưu ra đĩa để giữ cấu hình (vd đường dẫn MEmu bản Pro) qua các lần chạy.
    crate::persist_settings(&settings);
    Ok(settings)
}
