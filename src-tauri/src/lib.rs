//! Điểm khởi tạo ứng dụng Tauri. Lắp ráp state, chọn adapter memuc, đăng ký
//! command và khởi động poller nền.

mod adb;
mod commands;
mod crypto;
mod db;
#[cfg(test)]
mod e2e_real;
mod error;
mod fingerprint;
mod geo;
mod humanize;
mod memuc;
mod model;
mod orchestrator;
mod poller;
mod profile_ops;
mod queue;
mod runner;
mod snapshot;
mod state;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use adb::{AdbWorker, MockAdbWorker, RealAdbWorker};
use db::Db;
use geo::{HttpGeolocator, IpGeolocator};
use memuc::{MemucClient, MockMemuc, RealMemuc};
use model::AppSettings;
use snapshot::{LocalSnapshotStore, SnapshotStore};
use state::{AppState, InstanceMeta};

/// Chọn adapter: dùng MEmu thật nếu tìm thấy `memuc.exe`, ngược lại fallback mock
/// (để UI vẫn chạy được khi máy chưa cài MEmu — R-03).
fn build_memuc(settings: &AppSettings) -> Arc<dyn MemucClient> {
    #[cfg(feature = "mock-memuc")]
    {
        tracing::info!("Dùng MockMemuc (feature mock-memuc bật)");
        return Arc::new(MockMemuc::new());
    }

    #[cfg(not(feature = "mock-memuc"))]
    {
        match resolve_memuc(&settings.memu_path) {
            Some(p) => {
                tracing::info!(path = %p.display(), "Dùng RealMemuc");
                Arc::new(RealMemuc::new(p))
            }
            None => {
                tracing::warn!("Không tìm thấy memuc.exe — tạm dùng MockMemuc");
                Arc::new(MockMemuc::new())
            }
        }
    }
}

/// Giải đường dẫn tới `memuc.exe` từ setting: chấp nhận cả **thư mục cài MEmu**
/// (bất kỳ bản nào, kể cả bản Pro) lẫn đường dẫn `memuc.exe` trực tiếp; nếu không
/// có setting hợp lệ thì tự dò (discover). Cho phép trỏ tới build MEmu tùy chọn.
fn resolve_memuc(setting: &Option<String>) -> Option<PathBuf> {
    if let Some(s) = setting.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(s);
        // Trỏ thẳng memuc.exe.
        if p.is_file() {
            return Some(p);
        }
        // Trỏ thư mục cài → <dir>/memuc.exe.
        let candidate = p.join("memuc.exe");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    RealMemuc::discover()
}

/// Thư mục dữ liệu ứng dụng: %APPDATA%\com.mpm.manager (Windows).
fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(base).join("com.mpm.manager");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(error = %e, "Không tạo được thư mục dữ liệu");
        return None;
    }
    Some(dir)
}

fn db_path() -> Option<PathBuf> {
    Some(data_dir()?.join("mpm.db"))
}

fn settings_path() -> Option<PathBuf> {
    Some(data_dir()?.join("settings.json"))
}

/// Nạp settings từ đĩa (fallback mặc định) — giữ cấu hình qua các lần chạy.
fn load_settings() -> AppSettings {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
        .unwrap_or_default()
}

/// Ghi settings ra đĩa (gọi từ save_settings).
pub(crate) fn persist_settings(settings: &AppSettings) {
    if let Some(p) = settings_path() {
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            if let Err(e) = std::fs::write(&p, json) {
                tracing::warn!(error = %e, "Không ghi được settings.json");
            }
        }
    }
}

/// Chọn ADB Worker: dùng memuc thật nếu có, ngược lại Mock (dev/test).
fn build_adb(settings: &AppSettings) -> Arc<dyn AdbWorker> {
    match resolve_memuc(&settings.memu_path) {
        Some(p) => Arc::new(RealAdbWorker::new(p)),
        None => {
            tracing::warn!("Không tìm thấy memuc — ADB Worker dùng Mock");
            Arc::new(MockAdbWorker::new())
        }
    }
}

/// Nạp/sinh khóa mã hóa dùng chung cho snapshot **và** account_json (SEC-3).
/// Lưu tại `snapshot.key` trong thư mục dữ liệu. Lỗi → None (lưu không mã hóa).
fn load_enc_key() -> Option<crypto::Key32> {
    let dir = data_dir()?;
    match crypto::load_or_create_key(&dir.join("snapshot.key")) {
        Ok(k) => Some(k),
        Err(e) => {
            tracing::warn!(error = %e, "Không tạo được khóa mã hóa — lưu không mã hóa");
            None
        }
    }
}

/// Kho snapshot local dưới %APPDATA%\com.mpm.manager\snapshots (fallback: temp).
/// Snapshot được nén + **mã hóa AES-256-GCM** bằng khóa lưu tại `snapshot.key`.
fn build_store() -> Arc<dyn SnapshotStore> {
    let dir = data_dir();
    let root = dir
        .as_ref()
        .map(|d| d.join("snapshots"))
        .unwrap_or_else(|| std::env::temp_dir().join("mpm-snapshots"));
    let key = load_enc_key();
    match LocalSnapshotStore::new(&root, key) {
        Ok(s) => {
            tracing::info!(path = %root.display(), encrypted = key.is_some(), "Kho snapshot local");
            Arc::new(s)
        }
        Err(e) => {
            tracing::warn!(error = %e, "Không mở được kho snapshot — dùng thư mục temp");
            Arc::new(
                LocalSnapshotStore::new(std::env::temp_dir().join("mpm-snapshots"), key)
                    .expect("không tạo được kho snapshot tạm"),
            )
        }
    }
}

/// Mở SQLite và nạp metadata; trả về (db, metadata). Lỗi → fallback chỉ-bộ-nhớ.
fn init_db() -> (Option<Db>, HashMap<u32, InstanceMeta>) {
    let Some(path) = db_path() else {
        return (None, HashMap::new());
    };
    match Db::open_with_key(&path, load_enc_key()) {
        Ok(db) => {
            let meta = db.load_all().unwrap_or_default();
            tracing::info!(path = %path.display(), rows = meta.len(), "Đã mở SQLite metadata");
            (Some(db), meta)
        }
        Err(e) => {
            tracing::warn!(error = %e, "Không mở được SQLite — dùng bộ nhớ tạm");
            (None, HashMap::new())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings = load_settings();
    let memuc = build_memuc(&settings);
    // Tra IP→quốc gia thật qua ip-api.com (free, HTTP). Cache theo IP.
    let geo: Arc<dyn IpGeolocator> = Arc::new(HttpGeolocator::new());
    let adb = build_adb(&settings);
    let store = build_store();
    let (db, metadata) = init_db();
    let app_state: state::SharedState = Arc::new(AppState::new(
        memuc, geo, adb, store, settings, db, metadata,
    ));

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .manage(app_state.clone())
        .setup(move |app| {
            // Khởi động poller nền (§8.3).
            let handle = app.handle().clone();
            let state = app_state.clone();
            tauri::async_runtime::spawn(poller::run(handle, state));
            // Warm pool maintainer nền (opt-in qua settings).
            tauri::async_runtime::spawn(orchestrator::pool_maintainer(app_state.clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_instances,
            commands::start_instance,
            commands::stop_instance,
            commands::reboot_instance,
            commands::create_instance,
            commands::update_account,
            commands::update_note,
            commands::update_country,
            commands::backup_instance,
            commands::restore_instance,
            commands::provision_session,
            commands::teardown_session,
            commands::swap_account,
            commands::clone_from_base,
            commands::warm_pool_refill,
            commands::warm_pool_acquire,
            commands::warm_pool_size,
            commands::launch_instance,
            commands::install_apk,
            commands::install_tiktok,
            commands::disable_apps,
            commands::list_apps,
            commands::scan_emulator,
            commands::harden_vm,
            commands::human_tap,
            commands::human_swipe,
            commands::run_watch_session,
            commands::get_hardware,
            commands::remove_instance,
            commands::rename_instance,
            commands::bulk_action,
            commands::create_profile,
            commands::list_profiles,
            commands::update_profile,
            commands::run_profile,
            commands::stop_profile,
            commands::delete_profile,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("Lỗi khởi chạy ứng dụng MPM");
}
