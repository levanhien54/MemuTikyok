//! Điểm khởi tạo ứng dụng Tauri. Lắp ráp state, chọn adapter emulator, đăng ký
//! command và reconcile trạng thái lúc khởi động.

mod adb;
mod commands;
mod crypto;
mod db;
#[cfg(test)]
mod e2e_real;
mod emulator;
mod error;
mod fingerprint;
mod geo;
mod humanize;
mod logcap;
mod magisk;
mod model;
mod orchestrator;
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
use emulator::{EmulatorClient, MockClient, MumuClient};
use geo::{HttpGeolocator, IpGeolocator};
use model::AppSettings;
use snapshot::{LocalSnapshotStore, SnapshotStore};
use state::{AppState, InstanceMeta, ReloadableAdbWorker, ReloadableEmulatorClient};

struct DisabledSnapshotStore {
    reason: String,
}

#[async_trait::async_trait]
impl SnapshotStore for DisabledSnapshotStore {
    async fn put(
        &self,
        _key: &str,
        _file: &std::path::Path,
    ) -> error::AppResult<snapshot::StoredMeta> {
        Err(error::AppError::Io(self.reason.clone()))
    }

    async fn get(&self, _key: &str, _dst: &std::path::Path) -> error::AppResult<()> {
        Err(error::AppError::Io(self.reason.clone()))
    }

    async fn verify(&self, _key: &str, _sha256: &str) -> error::AppResult<bool> {
        Ok(false)
    }

    async fn delete(&self, _key: &str) -> error::AppResult<()> {
        Ok(())
    }
}

/// Chọn adapter: dùng MuMu thật nếu tìm thấy `MuMuManager.exe`, ngược lại fallback mock
/// (để UI vẫn chạy được khi máy chưa cài MuMu — R-03).
pub(crate) fn build_emulator(settings: &AppSettings) -> Arc<dyn EmulatorClient> {
    #[cfg(feature = "mock-emulator")]
    {
        let _ = settings;
        tracing::info!("Dùng MockClient (feature mock-emulator bật)");
        Arc::new(MockClient::new())
    }

    #[cfg(not(feature = "mock-emulator"))]
    {
        match resolve_emulator(&settings.mumu_path) {
            Some(p) => {
                tracing::info!(path = %p.display(), "Dùng MumuClient");
                Arc::new(MumuClient::new(p))
            }
            None => {
                tracing::warn!("Không tìm thấy MuMuManager.exe — tạm dùng MockClient");
                Arc::new(MockClient::new())
            }
        }
    }
}

/// Giải đường dẫn tới `MuMuManager.exe` từ setting: chấp nhận cả **thư mục cài MuMu**
/// (bất kỳ bản nào, kể cả bản Pro) lẫn đường dẫn `MuMuManager.exe` trực tiếp; nếu không
/// có setting hợp lệ thì tự dò (discover). Cho phép trỏ tới build MuMu tùy chọn.
fn resolve_emulator(setting: &Option<String>) -> Option<PathBuf> {
    if let Some(s) = setting.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(s);
        // Trỏ thẳng MuMuManager.exe.
        if p.is_file() {
            return Some(p);
        }
        // Trỏ thư mục cài → <dir>/MuMuManager.exe.
        let candidate = p.join("MuMuManager.exe");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    MumuClient::discover()
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
    let p = match settings_path() {
        Some(p) => p,
        None => return AppSettings::default(),
    };
    if let Ok(data) = std::fs::read_to_string(&p) {
        if let Ok(s) = serde_json::from_str(&data) {
            return s;
        }
    }
    AppSettings::default()
}

/// Ghi settings xuống đĩa. Dùng khi user update từ UI.
pub(crate) fn persist_settings(settings: &AppSettings) {
    if let Some(p) = settings_path() {
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            if let Err(e) = std::fs::write(&p, json) {
                tracing::warn!(error = %e, "Không ghi được settings.json");
            }
        }
    }
}

/// Chọn ADB Worker: dùng MuMuManager thật nếu có, ngược lại Mock (dev/test).
pub(crate) fn build_adb(settings: &AppSettings) -> Arc<dyn AdbWorker> {
    match resolve_emulator(&settings.mumu_path) {
        Some(p) => Arc::new(RealAdbWorker::new(p)),
        None => {
            tracing::warn!("Không tìm thấy emulator — ADB Worker dùng Mock");
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
            match LocalSnapshotStore::new(std::env::temp_dir().join("mpm-snapshots"), key) {
                Ok(s) => Arc::new(s),
                Err(e2) => {
                    tracing::error!(error = %e2, "Không tạo được kho snapshot tạm — backup sẽ bị vô hiệu hóa");
                    Arc::new(DisabledSnapshotStore {
                        reason: format!("Không mở được kho snapshot: {e2}"),
                    })
                }
            }
        }
    }
}

/// Trích binary magisk (resetprop) từ Magisk APK cấu hình trong settings, cache vào
/// thư mục dữ liệu. Trả `None` nếu chưa cấu hình / APK hỏng → model không khóa được.
/// `pub(crate)` để `save_settings` áp lại NGAY khi người dùng đổi đường dẫn (không đợi restart).
pub(crate) fn init_magisk_bin(settings: &AppSettings) -> Option<PathBuf> {
    let apk = settings
        .magisk_apk_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    if !std::path::Path::new(apk).is_file() {
        tracing::warn!(apk, "Magisk APK cấu hình không tồn tại — bỏ qua khóa model");
        return None;
    }
    let cache = data_dir()?.join("magisk");
    match crate::magisk::ensure_binary(apk, &cache) {
        Some(p) => {
            tracing::info!(path = %p.display(), "Sẵn sàng resetprop từ Magisk");
            Some(p)
        }
        None => {
            tracing::warn!("Không trích được resetprop từ APK — bỏ qua khóa model");
            None
        }
    }
}

/// Khởi tạo SQLite DB + lấy InstanceMeta nếu có.
fn init_db(key: Option<crypto::Key32>) -> (Option<Db>, HashMap<u32, InstanceMeta>) {
    match db_path() {
        Some(p) => {
            let db = match Db::open_with_key(&p, key) {
                Ok(db) => db,
                Err(e) => {
                    tracing::error!(path = %p.display(), error = %e, "Không mở được DB persistent — dùng in-memory");
                    return open_memory_db();
                }
            };
            let meta = match db.load_all() {
                Ok(meta) => meta,
                Err(e) => {
                    tracing::error!(path = %p.display(), error = %e, "Không load được metadata DB — tắt DB persistent để tránh ghi đè dữ liệu cũ");
                    return (None, HashMap::new());
                }
            };
            if let Err(e) = db.load_profiles() {
                tracing::error!(path = %p.display(), error = %e, "Không giải mã/load được profile DB — tắt DB persistent để tránh ghi đè credential");
                return (None, HashMap::new());
            }
            (Some(db), meta)
        }
        None => {
            tracing::warn!("Không có data_dir, dùng DB trong RAM");
            open_memory_db()
        }
    }
}

fn open_memory_db() -> (Option<Db>, HashMap<u32, InstanceMeta>) {
    match Db::open_with_key(std::path::Path::new(":memory:"), None) {
        Ok(db) => (Some(db), HashMap::new()),
        Err(e) => {
            tracing::error!(error = %e, "Không mở được in-memory DB — chạy không persistence");
            (None, HashMap::new())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Bắt log của app (tracing) vào ring buffer cho LogsView + stderr. Phải cài TRƯỚC
    // mọi tracing::* để không rơi log khởi tạo.
    let log_buffer = logcap::init();

    let settings = load_settings();
    let emulator_inner = build_emulator(&settings);
    let emulator_reload = Arc::new(ReloadableEmulatorClient::new(emulator_inner));
    let emulator: Arc<dyn EmulatorClient> = emulator_reload.clone();
    // Tra IP→quốc gia thật qua ip-api.com (free, HTTP). Cache theo IP.
    let geo: Arc<dyn IpGeolocator> = Arc::new(HttpGeolocator::new());
    let adb_inner = build_adb(&settings);
    let adb_reload = Arc::new(ReloadableAdbWorker::new(adb_inner));
    let adb: Arc<dyn AdbWorker> = adb_reload.clone();

    // Khoá mã hoá cho DB và snapshot
    let enc_key = load_enc_key();

    let store = build_store();
    let (db, metadata) = init_db(enc_key);
    // Trích binary magisk TRƯỚC khi move `settings` vào AppState.
    let magisk_bin = init_magisk_bin(&settings);
    let app_state: state::SharedState = Arc::new(AppState::new_reloadable(
        emulator,
        emulator_reload,
        geo,
        adb,
        adb_reload,
        store,
        settings,
        db,
        metadata,
    ));
    app_state.set_magisk_bin(magisk_bin);
    let reconcile_state = app_state.clone();

    // KHÔNG dùng tauri-plugin-log: logcap (tracing-subscriber) đã sở hữu global logger
    // (bắc cầu `log`→tracing qua try_init) + LogsView đọc ring buffer. Nếu thêm plugin-log,
    // nó cố set logger LẦN 2 → PluginInitialization panic → app tắt câm khi khởi động.
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .manage(log_buffer)
        .setup(move |_app| {
            // RECONCILE khởi động: dọn VM mồ côi từ phiên trước (crash/tắt đột ngột).
            tauri::async_runtime::spawn(async move {
                let n = profile_ops::reconcile_startup(&reconcile_state).await;
                if n > 0 {
                    tracing::info!(cleaned = n, "Reconcile khởi động: đã dọn VM mồ côi");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Vòng đời PROFILE (disposable: profile = dữ liệu, VM = tạo mới mỗi lần chạy).
            commands::create_profile,
            commands::list_profiles,
            commands::update_profile,
            commands::delete_profile,
            commands::run_profile,
            commands::stop_profile,
            commands::scan_emulator,
            commands::run_watch_session,
            commands::upload_video_to_vm,
            // Settings
            commands::get_settings,
            commands::save_settings,
            // Logs
            commands::get_logs,
        ])
        .run(tauri::generate_context!())
        .expect("Lỗi chạy tauri app");
}
