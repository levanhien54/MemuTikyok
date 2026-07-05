//! Tauri commands — biên giới IPC được UI gọi qua `invoke` (§8.4 SRS).
//!
//! Kiến trúc DISPOSABLE (profile = dữ liệu bền; VM tạo mới mỗi lần chạy rồi hủy):
//! biên IPC chỉ phơi bày vòng đời PROFILE + tiện ích trên VM đang chạy + cài đặt.
//! Lõi nghiệp vụ profile nằm ở `crate::profile_ops` (test được trực tiếp, kể cả E2E thật).

use tauri::{AppHandle, Emitter, State};

use crate::error::AppResult;
use crate::model::{AccountProfile, AppSettings, EmulatorTell, ProfileView, SnapshotRecord};
use crate::state::SharedState;

// ── Vòng đời PROFILE — lệnh dưới đây chỉ là adapter mỏng của `crate::profile_ops` ──

/// Tạo PROFILE mới — CHỈ ghi dữ liệu (account + fingerprint), KHÔNG tạo VM.
#[tauri::command]
pub async fn create_profile(
    account: AccountProfile,
    note: Option<String>,
    country: Option<String>,
    state: State<'_, SharedState>,
) -> AppResult<String> {
    crate::profile_ops::create(state.inner(), account, note, country).await
}

/// Danh sách profile + trạng thái runtime (đang chạy trên VM nào).
#[tauri::command]
pub async fn list_profiles(state: State<'_, SharedState>) -> AppResult<Vec<ProfileView>> {
    Ok(crate::profile_ops::list(state.inner()).await)
}

/// Cập nhật account/ghi chú/quốc gia của profile (giữ nguyên username-key).
#[tauri::command]
pub async fn update_profile(
    username: String,
    account: AccountProfile,
    note: String,
    country: Option<String>,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    crate::profile_ops::update(state.inner(), &username, account, note, country).await
}

/// CHẠY profile: cấp VM sạch, áp fingerprint + cài TikTok + restore session theo
/// username, mở TikTok. Giữ vm_index ↔ username. Chặn khi vượt tối đa 5 VM.
#[tauri::command]
pub async fn run_profile(username: String, state: State<'_, SharedState>) -> AppResult<u32> {
    crate::profile_ops::run(state.inner(), &username).await
}

/// DỪNG profile: backup session → HỦY VM (disposable). Trả snapshot nếu có.
#[tauri::command]
pub async fn stop_profile(
    username: String,
    state: State<'_, SharedState>,
) -> AppResult<Option<SnapshotRecord>> {
    crate::profile_ops::stop(state.inner(), &username).await
}

/// XÓA profile: nếu đang chạy thì teardown trước, rồi xóa bản ghi.
#[tauri::command]
pub async fn delete_profile(username: String, state: State<'_, SharedState>) -> AppResult<()> {
    crate::profile_ops::delete(state.inner(), &username).await
}

// ── Tiện ích trên VM đang chạy của profile ──

/// Scan dấu vết emulator của VM đang chạy (chẩn đoán chống phát hiện MÁY ẢO).
#[tauri::command]
pub async fn scan_emulator(
    index: u32,
    state: State<'_, SharedState>,
) -> AppResult<Vec<EmulatorTell>> {
    state.adb.scan_emulator_tells(index).await
}

/// Chạy phiên "xem feed" TikTok ở NỀN (warm-up). Trả ngay; khi xong phát sự kiện
/// `automation:done` (SessionReport) hoặc `automation:error`.
#[tauri::command]
pub async fn run_watch_session(
    index: u32,
    config: Option<crate::runner::WatchConfig>,
    app: AppHandle,
    state: State<'_, SharedState>,
) -> AppResult<()> {
    let st = state.inner().clone();
    let cfg = config.unwrap_or_default();
    tauri::async_runtime::spawn(async move {
        match crate::runner::run_watch_session(&st, index, cfg).await {
            Ok(rep) => {
                let _ = app.emit("automation:done", rep);
            }
            Err(e) => {
                let _ = app.emit(
                    "automation:error",
                    serde_json::json!({ "index": index, "error": e.to_string() }),
                );
            }
        }
    });
    Ok(())
}

// ── Cài đặt ──

#[tauri::command]
pub async fn get_settings(state: State<'_, SharedState>) -> AppResult<AppSettings> {
    Ok(state.settings.lock().await.clone())
}

/// Log ứng dụng gần nhất (ring buffer) để LogsView hiển thị — chẩn đoán khi Chạy lỗi.
#[tauri::command]
pub async fn get_logs(logs: State<'_, crate::logcap::LogBuffer>) -> AppResult<Vec<String>> {
    Ok(logs.lock().unwrap().iter().cloned().collect())
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
    // Áp NGAY thay đổi Magisk APK (trích lại binary + set) — nếu không, đổi/trỏ đường dẫn
    // trong Cài đặt sẽ KHÔNG khóa được model cho tới khi khởi động lại app. Trỏ rỗng → None
    // (tắt khóa model). Cache theo mtime nên gọi lại mỗi lần lưu là rẻ khi APK không đổi.
    state.set_magisk_bin(crate::init_magisk_bin(&settings));
    Ok(settings)
}
