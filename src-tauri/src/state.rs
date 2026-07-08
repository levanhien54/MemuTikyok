//! State dùng chung của ứng dụng (§8.3 SRS). Giữ adapter emulator, hàng đợi lệnh,
//! registry instance, settings, metadata (persist SQLite) và geolocator.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

use crate::adb::AdbWorker;
use crate::db::Db;
use crate::emulator::EmulatorClient;
use crate::geo::IpGeolocator;
use crate::model::{
    AccountProfile, AppSettings, EmulatorTell, FingerprintLockStatus, HardwareProfile, Instance,
    Profile, ProvisionHealth, SnapshotMeta,
};
use crate::queue::CommandQueue;
use crate::snapshot::SnapshotStore;

/// vm_index "đặt chỗ": một lần `run` đang provision (chưa có VM thật). Chiếm slot
/// trong cổng tối đa để chống đua, nhưng KHÔNG hiển thị là đang chạy. `u32::MAX` là
/// index bất khả (emulator không bao giờ cấp) nên an toàn làm cờ.
pub const RESERVED_VM: u32 = u32::MAX;

/// Kết quả đặt chỗ slot chạy (nguyên tử) — xem `AppState::reserve_run_slot`.
pub enum RunSlot {
    /// Profile đã chạy trên VM này rồi (trả về ngay, idempotent).
    AlreadyRunning(u32),
    /// Một lần `run` khác đang provision profile này — chưa xong.
    Pending,
    /// Profile đang backup+dừng; VM vẫn chiếm slot cho tới khi remove xong.
    Stopping,
    /// Đã đạt tối đa số VM chạy đồng thời.
    AtCapacity,
    /// Đã đặt chỗ thành công — caller được phép provision.
    Reserved,
}

pub enum TeardownSlot {
    Ready(u32),
    NotRunning,
    Pending,
    AlreadyStopping,
}

/// Metadata do MPM tự quản cho từng VM (không thuộc emulator). Persist vào SQLite.
#[derive(Debug, Clone, Default)]
pub struct InstanceMeta {
    pub account: Option<AccountProfile>,
    pub last_launched_at: Option<i64>,
    pub country: Option<String>,
    pub note: String,
    /// Fingerprint gắn với tài khoản — sinh 1 lần, lưu DB, áp lại mỗi lần khởi chạy.
    pub hardware: Option<HardwareProfile>,
}

pub struct ReloadableEmulatorClient {
    inner: RwLock<Arc<dyn EmulatorClient>>,
}

impl ReloadableEmulatorClient {
    pub fn new(inner: Arc<dyn EmulatorClient>) -> Self {
        Self {
            inner: RwLock::new(inner),
        }
    }

    fn current(&self) -> Arc<dyn EmulatorClient> {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn replace(&self, inner: Arc<dyn EmulatorClient>) {
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = inner;
    }
}

#[async_trait::async_trait]
impl EmulatorClient for ReloadableEmulatorClient {
    async fn list_instances(&self) -> crate::error::AppResult<Vec<Instance>> {
        let inner = self.current();
        inner.list_instances().await
    }

    async fn start(&self, index: u32) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.start(index).await
    }

    async fn stop(&self, index: u32) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.stop(index).await
    }

    async fn create(&self) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.create().await
    }

    async fn remove(&self, index: u32) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.remove(index).await
    }

    async fn set_config(&self, index: u32, key: &str, value: &str) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.set_config(index, key, value).await
    }

    async fn set_resolution(
        &self,
        index: u32,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.set_resolution(index, width, height, dpi).await
    }
}

pub struct ReloadableAdbWorker {
    inner: RwLock<Arc<dyn AdbWorker>>,
}

impl ReloadableAdbWorker {
    pub fn new(inner: Arc<dyn AdbWorker>) -> Self {
        Self {
            inner: RwLock::new(inner),
        }
    }

    fn current(&self) -> Arc<dyn AdbWorker> {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn replace(&self, inner: Arc<dyn AdbWorker>) {
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = inner;
    }
}

#[async_trait::async_trait]
impl AdbWorker for ReloadableAdbWorker {
    async fn backup(
        &self,
        idx: u32,
        pkg: &str,
        out: &Path,
    ) -> crate::error::AppResult<SnapshotMeta> {
        let inner = self.current();
        inner.backup(idx, pkg, out).await
    }

    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.restore(idx, pkg, archive).await
    }

    async fn apk_version(&self, idx: u32, pkg: &str) -> crate::error::AppResult<String> {
        let inner = self.current();
        inner.apk_version(idx, pkg).await
    }

    async fn apply_android_id(&self, idx: u32, android_id: &str) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.apply_android_id(idx, android_id).await
    }

    async fn apply_display_profile(
        &self,
        idx: u32,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> crate::error::AppResult<bool> {
        let inner = self.current();
        inner.apply_display_profile(idx, width, height, dpi).await
    }

    async fn wait_boot_completed(&self, idx: u32) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.wait_boot_completed(idx).await
    }

    async fn start_app(&self, idx: u32, pkg: &str) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.start_app(idx, pkg).await
    }

    async fn install_apk(&self, idx: u32, apk_path: &str) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.install_apk(idx, apk_path).await
    }

    async fn disable_app(&self, idx: u32, pkg: &str) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.disable_app(idx, pkg).await
    }

    async fn scan_emulator_tells(&self, idx: u32) -> crate::error::AppResult<Vec<EmulatorTell>> {
        let inner = self.current();
        inner.scan_emulator_tells(idx).await
    }

    async fn harden(&self, idx: u32) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.harden(idx).await
    }

    async fn push_resetprop(&self, idx: u32, local_bin: &str) -> crate::error::AppResult<bool> {
        let inner = self.current();
        inner.push_resetprop(idx, local_bin).await
    }

    async fn lock_device_identity(
        &self,
        idx: u32,
        hw: &HardwareProfile,
    ) -> crate::error::AppResult<bool> {
        let inner = self.current();
        inner.lock_device_identity(idx, hw).await
    }

    async fn human_tap(&self, idx: u32, x: i32, y: i32) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.human_tap(idx, x, y).await
    }

    async fn human_swipe(
        &self,
        idx: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
    ) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.human_swipe(idx, x0, y0, x1, y1).await
    }

    async fn upload_media(&self, idx: u32, local_path: &str) -> crate::error::AppResult<()> {
        let inner = self.current();
        inner.upload_media(idx, local_path).await
    }
}

pub struct AppState {
    pub emulator: Arc<dyn EmulatorClient>,
    emulator_reload: Option<Arc<ReloadableEmulatorClient>>,
    pub geo: Arc<dyn IpGeolocator>,
    pub adb: Arc<dyn AdbWorker>,
    adb_reload: Option<Arc<ReloadableAdbWorker>>,
    pub store: Arc<dyn SnapshotStore>,
    pub queue: CommandQueue,
    pub settings: Mutex<AppSettings>,
    /// Metadata theo index VM (bộ nhớ, đồng bộ với SQLite).
    pub metadata: Mutex<HashMap<u32, InstanceMeta>>,
    /// Kết nối SQLite; None nếu không mở được (fallback chỉ-bộ-nhớ).
    pub db: Option<Db>,
    /// Khóa tuần tự hóa thao tác "tạo VM rồi nhận diện index mới" — tránh
    /// hai lần tạo song song cùng chọn nhầm một index (race + tái dùng index).
    pub create_lock: Mutex<()>,
    /// VM đang chạy phiên automation — chặn phiên TRÙNG trên cùng VM.
    pub running_sessions: Mutex<HashSet<u32>>,
    /// PROFILE (dữ liệu bền, khóa theo username) — tách khỏi vm_index. Nguồn sự thật
    /// cho danh sách tài khoản; VM chỉ là pool tạm để chạy profile.
    pub profiles: Mutex<HashMap<String, Profile>>,
    /// Ánh xạ profile ĐANG CHẠY → vm_index (bộ nhớ, không persist). Dùng **std Mutex**
    /// (không giữ qua await ở đâu cả) để RAII guard nhả chỗ được TRONG Drop (đồng bộ).
    pub running_profiles: std::sync::Mutex<HashMap<String, u32>>,
    pub stopping_profiles: std::sync::Mutex<HashSet<String>>,
    /// Binary magisk (resetprop) đã trích từ Magisk APK — set lúc khởi động nếu
    /// `settings.magisk_apk_path` có. None = không khóa được model (thiếu Magisk APK).
    pub magisk_bin: std::sync::Mutex<Option<std::path::PathBuf>>,
    pub fingerprint_locks: Mutex<HashMap<u32, FingerprintLockStatus>>,
    pub fixable_tells: Mutex<HashMap<u32, Vec<String>>>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        emulator: Arc<dyn EmulatorClient>,
        geo: Arc<dyn IpGeolocator>,
        adb: Arc<dyn AdbWorker>,
        store: Arc<dyn SnapshotStore>,
        settings: AppSettings,
        db: Option<Db>,
        metadata: HashMap<u32, InstanceMeta>,
    ) -> Self {
        let queue = CommandQueue::new(settings.max_concurrency as usize);
        // Nạp profile từ DB → map theo username.
        let profiles = db
            .as_ref()
            .and_then(|d| d.load_profiles().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.username.clone(), p))
            .collect();
        Self {
            emulator,
            emulator_reload: None,
            geo,
            adb,
            adb_reload: None,
            store,
            queue,
            settings: Mutex::new(settings),
            metadata: Mutex::new(metadata),
            db,
            create_lock: Mutex::new(()),
            running_sessions: Mutex::new(HashSet::new()),
            profiles: Mutex::new(profiles),
            running_profiles: std::sync::Mutex::new(HashMap::new()),
            stopping_profiles: std::sync::Mutex::new(HashSet::new()),
            magisk_bin: std::sync::Mutex::new(None),
            fingerprint_locks: Mutex::new(HashMap::new()),
            fixable_tells: Mutex::new(HashMap::new()),
        }
    }

    /// Constructor production: adapter trait object ben ngoai on dinh, inner co the swap runtime.
    #[allow(clippy::too_many_arguments)]
    pub fn new_reloadable(
        emulator: Arc<dyn EmulatorClient>,
        emulator_reload: Arc<ReloadableEmulatorClient>,
        geo: Arc<dyn IpGeolocator>,
        adb: Arc<dyn AdbWorker>,
        adb_reload: Arc<ReloadableAdbWorker>,
        store: Arc<dyn SnapshotStore>,
        settings: AppSettings,
        db: Option<Db>,
        metadata: HashMap<u32, InstanceMeta>,
    ) -> Self {
        let mut state = Self::new(emulator, geo, adb, store, settings, db, metadata);
        state.emulator_reload = Some(emulator_reload);
        state.adb_reload = Some(adb_reload);
        state
    }

    pub fn reload_clients(
        &self,
        emulator: Arc<dyn EmulatorClient>,
        adb: Arc<dyn AdbWorker>,
    ) -> bool {
        match (&self.emulator_reload, &self.adb_reload) {
            (Some(emulator_reload), Some(adb_reload)) => {
                emulator_reload.replace(emulator);
                adb_reload.replace(adb);
                true
            }
            _ => false,
        }
    }

    /// Dat duong dan binary magisk (resetprop) da trich.
    pub fn set_magisk_bin(&self, path: Option<std::path::PathBuf>) {
        *self.magisk_bin.lock().unwrap() = path;
    }

    /// Đường dẫn binary magisk (nếu có) — provision đẩy vào VM để khóa model.
    pub fn magisk_bin(&self) -> Option<std::path::PathBuf> {
        self.magisk_bin.lock().unwrap().clone()
    }

    /// Ghi/cập nhật profile vào bộ nhớ + DB.
    pub async fn set_fingerprint_lock_status(&self, index: u32, status: FingerprintLockStatus) {
        self.fingerprint_locks.lock().await.insert(index, status);
    }

    pub async fn fingerprint_lock_status(&self, index: u32) -> FingerprintLockStatus {
        self.fingerprint_locks
            .lock()
            .await
            .get(&index)
            .cloned()
            .unwrap_or_else(FingerprintLockStatus::not_attempted)
    }

    pub async fn set_fixable_tells(&self, index: u32, tells: Vec<String>) {
        self.fixable_tells.lock().await.insert(index, tells);
    }

    pub async fn provision_health(&self, index: u32) -> ProvisionHealth {
        ProvisionHealth {
            fingerprint_lock: self.fingerprint_lock_status(index).await,
            fixable_tells: self
                .fixable_tells
                .lock()
                .await
                .get(&index)
                .cloned()
                .unwrap_or_default(),
        }
    }

    pub async fn upsert_profile(&self, profile: Profile) {
        if let Some(db) = &self.db {
            let _ = db.upsert_profile(&profile);
        }
        self.profiles
            .lock()
            .await
            .insert(profile.username.clone(), profile);
    }

    /// Danh sách profile (đã sắp theo thời điểm tạo).
    pub async fn list_profiles(&self) -> Vec<Profile> {
        let mut v: Vec<Profile> = self.profiles.lock().await.values().cloned().collect();
        v.sort_by_key(|p| p.created_at);
        v
    }

    pub async fn get_profile(&self, username: &str) -> Option<Profile> {
        self.profiles.lock().await.get(username).cloned()
    }

    pub async fn delete_profile(&self, username: &str) {
        self.profiles.lock().await.remove(username);
        self.running_profiles.lock().unwrap().remove(username);
        self.stopping_profiles.lock().unwrap().remove(username);
        if let Some(db) = &self.db {
            let _ = db.delete_profile(username);
            let _ = db.remove_running(username);
        }
    }

    /// vm_index ĐANG CHẠY profile này (bỏ qua slot đặt chỗ `RESERVED_VM` — provision
    /// chưa xong thì chưa coi là đang chạy, tránh hiện "VM #4294967295").
    pub async fn running_vm_of(&self, username: &str) -> Option<u32> {
        self.running_profiles
            .lock()
            .unwrap()
            .get(username)
            .copied()
            .filter(|&v| v != RESERVED_VM)
    }

    /// True nếu profile đang GIỮA giai đoạn provision (đã đặt chỗ `RESERVED_VM`, VM thật
    /// chưa xong). Dùng để `stop`/`delete` từ chối thao tác khi run đang bay.
    #[cfg(test)]
    pub async fn is_reserved(&self, username: &str) -> bool {
        self.running_profiles.lock().unwrap().get(username).copied() == Some(RESERVED_VM)
    }

    pub async fn is_stopping(&self, username: &str) -> bool {
        self.stopping_profiles.lock().unwrap().contains(username)
    }

    pub async fn set_running_profile(&self, username: &str, vm_index: u32) {
        self.stopping_profiles.lock().unwrap().remove(username);
        self.running_profiles
            .lock()
            .unwrap()
            .insert(username.to_string(), vm_index);
        // Persist để reconcile được sau crash (chỉ ghi idx THẬT, không ghi RESERVED).
        if let Some(db) = &self.db {
            let _ = db.record_running(username, vm_index);
        }
    }

    pub async fn clear_running_profile(&self, username: &str) {
        self.running_profiles.lock().unwrap().remove(username);
        self.stopping_profiles.lock().unwrap().remove(username);
        if let Some(db) = &self.db {
            let _ = db.remove_running(username);
        }
    }

    pub fn record_running_marker(&self, username: &str, vm_index: u32) {
        if let Some(db) = &self.db {
            let _ = db.record_running(username, vm_index);
        }
    }

    pub fn clear_running_marker(&self, username: &str) {
        if let Some(db) = &self.db {
            let _ = db.remove_running(username);
        }
    }

    /// NGUYÊN TỬ: kiểm idempotency + cổng tối đa RỒI đặt chỗ slot — tất cả dưới MỘT
    /// khóa. Chống đua (R): N lần `run` song song không cùng vượt cổng; cùng username
    /// không provision đôi. Caller nhận `Reserved` phải finalize bằng `set_running_profile`
    /// (thành công) hoặc `clear_running_profile` (lỗi) để nhả chỗ.
    pub async fn reserve_run_slot(&self, username: &str, max: usize) -> RunSlot {
        if self.stopping_profiles.lock().unwrap().contains(username) {
            return RunSlot::Stopping;
        }
        let mut g = self.running_profiles.lock().unwrap();
        match g.get(username).copied() {
            Some(v) if v != RESERVED_VM => return RunSlot::AlreadyRunning(v),
            Some(_) => return RunSlot::Pending,
            None => {}
        }
        if g.len() >= max {
            return RunSlot::AtCapacity;
        }
        g.insert(username.to_string(), RESERVED_VM);
        RunSlot::Reserved
    }

    pub async fn begin_teardown_profile(&self, username: &str) -> TeardownSlot {
        {
            let mut stopping = self.stopping_profiles.lock().unwrap();
            if !stopping.insert(username.to_string()) {
                return TeardownSlot::AlreadyStopping;
            }
        }

        let idx = match self.running_profiles.lock().unwrap().get(username).copied() {
            Some(v) if v == RESERVED_VM => {
                self.stopping_profiles.lock().unwrap().remove(username);
                return TeardownSlot::Pending;
            }
            Some(v) => v,
            None => {
                self.stopping_profiles.lock().unwrap().remove(username);
                return TeardownSlot::NotRunning;
            }
        };
        if idx == RESERVED_VM {
            return TeardownSlot::AlreadyStopping;
        }
        TeardownSlot::Ready(idx)
    }

    pub async fn finish_teardown_profile(&self, username: &str) {
        self.running_profiles.lock().unwrap().remove(username);
        self.stopping_profiles.lock().unwrap().remove(username);
        if let Some(db) = &self.db {
            let _ = db.remove_running(username);
        }
    }

    pub async fn abort_teardown_profile(&self, username: &str) {
        self.stopping_profiles.lock().unwrap().remove(username);
    }

    /// Ghi write-through xuống SQLite (best-effort; lỗi chỉ log).
    fn persist(&self, index: u32, entry: &InstanceMeta) {
        if let Some(db) = &self.db {
            if let Err(e) = db.upsert(index, entry) {
                tracing::warn!(error = %e, index, "Ghi metadata vào SQLite thất bại");
            }
        }
    }

    pub async fn mark_launched(&self, index: u32) {
        let entry = {
            let mut meta = self.metadata.lock().await;
            let e = meta.entry(index).or_default();
            e.last_launched_at = Some(now_ms());
            e.clone()
        };
        self.persist(index, &entry);
    }

    /// PROFILE đang chạy trên VM `idx` (tra ngược `running_profiles` → `get_profile`).
    /// Dùng để automation lấy độ phân giải THẬT của profile (mô hình profile không ghi
    /// InstanceMeta.hardware per-index nữa nên phải tra qua profile).
    pub async fn profile_on_vm(&self, idx: u32) -> Option<Profile> {
        let username = {
            let g = self.running_profiles.lock().unwrap();
            g.iter().find(|(_, &v)| v == idx).map(|(u, _)| u.clone())
        };
        match username {
            Some(u) => self.get_profile(&u).await,
            None => None,
        }
    }

    pub async fn forget(&self, index: u32) {
        self.metadata.lock().await.remove(&index);
        self.fingerprint_locks.lock().await.remove(&index);
        self.fixable_tells.lock().await.remove(&index);
        if let Some(db) = &self.db {
            let _ = db.delete(index);
        }
    }
}

pub type SharedState = Arc<AppState>;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::MockClient;

    #[tokio::test]
    async fn reloadable_emulator_swaps_inner_client() {
        let first = Arc::new(MockClient::new());
        first.create().await.unwrap();
        let reloadable = ReloadableEmulatorClient::new(first);

        assert_eq!(reloadable.list_instances().await.unwrap().len(), 1);

        reloadable.replace(Arc::new(MockClient::new()));

        assert!(
            reloadable.list_instances().await.unwrap().is_empty(),
            "after reload, calls must use the replacement emulator client"
        );
    }
}
