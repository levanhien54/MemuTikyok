//! State dùng chung của ứng dụng (§8.3 SRS). Giữ adapter emulator, hàng đợi lệnh,
//! registry instance, settings, metadata (persist SQLite) và geolocator.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

use crate::adb::AdbWorker;
use crate::db::Db;
use crate::emulator::EmulatorClient;
use crate::geo::IpGeolocator;
use crate::model::{AccountProfile, AppSettings, HardwareProfile, Profile};
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
    /// Đã đạt tối đa số VM chạy đồng thời.
    AtCapacity,
    /// Đã đặt chỗ thành công — caller được phép provision.
    Reserved,
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

pub struct AppState {
    pub emulator: Arc<dyn EmulatorClient>,
    pub geo: Arc<dyn IpGeolocator>,
    pub adb: Arc<dyn AdbWorker>,
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
    /// Binary magisk (resetprop) đã trích từ Magisk APK — set lúc khởi động nếu
    /// `settings.magisk_apk_path` có. None = không khóa được model (thiếu Magisk APK).
    pub magisk_bin: std::sync::Mutex<Option<std::path::PathBuf>>,
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
            geo,
            adb,
            store,
            queue,
            settings: Mutex::new(settings),
            metadata: Mutex::new(metadata),
            db,
            create_lock: Mutex::new(()),
            running_sessions: Mutex::new(HashSet::new()),
            profiles: Mutex::new(profiles),
            running_profiles: std::sync::Mutex::new(HashMap::new()),
            magisk_bin: std::sync::Mutex::new(None),
        }
    }

    /// Đặt đường dẫn binary magisk (resetprop) đã trích (gọi lúc khởi động).
    pub fn set_magisk_bin(&self, path: Option<std::path::PathBuf>) {
        *self.magisk_bin.lock().unwrap() = path;
    }

    /// Đường dẫn binary magisk (nếu có) — provision đẩy vào VM để khóa model.
    pub fn magisk_bin(&self) -> Option<std::path::PathBuf> {
        self.magisk_bin.lock().unwrap().clone()
    }

    /// Ghi/cập nhật profile vào bộ nhớ + DB.
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
    pub async fn is_reserved(&self, username: &str) -> bool {
        self.running_profiles.lock().unwrap().get(username).copied() == Some(RESERVED_VM)
    }

    pub async fn set_running_profile(&self, username: &str, vm_index: u32) {
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
        if let Some(db) = &self.db {
            let _ = db.remove_running(username);
        }
    }

    /// NGUYÊN TỬ: kiểm idempotency + cổng tối đa RỒI đặt chỗ slot — tất cả dưới MỘT
    /// khóa. Chống đua (R): N lần `run` song song không cùng vượt cổng; cùng username
    /// không provision đôi. Caller nhận `Reserved` phải finalize bằng `set_running_profile`
    /// (thành công) hoặc `clear_running_profile` (lỗi) để nhả chỗ.
    pub async fn reserve_run_slot(&self, username: &str, max: usize) -> RunSlot {
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

    /// NGUYÊN TỬ lấy-và-xóa vm_index đang chạy (bỏ qua slot đặt chỗ). Chỉ MỘT caller
    /// thắng entry → chống teardown đôi khi hai `stop`/`delete` chạy song song.
    pub async fn take_running_vm(&self, username: &str) -> Option<u32> {
        let taken = {
            let mut g = self.running_profiles.lock().unwrap();
            match g.get(username).copied() {
                Some(v) if v != RESERVED_VM => {
                    g.remove(username);
                    Some(v)
                }
                _ => None,
            }
        };
        if taken.is_some() {
            if let Some(db) = &self.db {
                let _ = db.remove_running(username);
            }
        }
        taken
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
