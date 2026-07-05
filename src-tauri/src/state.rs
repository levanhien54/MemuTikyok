//! State dùng chung của ứng dụng (§8.3 SRS). Giữ adapter memuc, hàng đợi lệnh,
//! registry instance, settings, metadata (persist SQLite) và geolocator.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

use crate::adb::AdbWorker;
use crate::db::Db;
use crate::geo::IpGeolocator;
use crate::memuc::MemucClient;
use crate::model::{AccountProfile, AppSettings, HardwareProfile, Instance};
use crate::queue::CommandQueue;
use crate::snapshot::SnapshotStore;

/// Metadata do MPM tự quản cho từng VM (không thuộc memuc). Persist vào SQLite.
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
    pub memuc: Arc<dyn MemucClient>,
    pub geo: Arc<dyn IpGeolocator>,
    pub adb: Arc<dyn AdbWorker>,
    pub store: Arc<dyn SnapshotStore>,
    pub queue: CommandQueue,
    /// Snapshot instance gần nhất (nguồn cấp cho UI, cập nhật bởi poller).
    pub registry: Mutex<Vec<Instance>>,
    pub settings: Mutex<AppSettings>,
    /// Metadata theo index VM (bộ nhớ, đồng bộ với SQLite).
    pub metadata: Mutex<HashMap<u32, InstanceMeta>>,
    /// Kết nối SQLite; None nếu không mở được (fallback chỉ-bộ-nhớ).
    pub db: Option<Db>,
    /// Warm pool: các VM đã clone + boot sẵn, chờ gán tài khoản (0s cold-boot).
    pub pool: Mutex<VecDeque<u32>>,
    /// Khóa tuần tự hóa thao tác "tạo/clone VM rồi nhận diện index mới" — tránh
    /// hai lần tạo song song cùng chọn nhầm một index (race + tái dùng index).
    pub create_lock: Mutex<()>,
    /// VM đang chạy phiên automation — chặn phiên TRÙNG trên cùng VM.
    pub running_sessions: Mutex<HashSet<u32>>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        memuc: Arc<dyn MemucClient>,
        geo: Arc<dyn IpGeolocator>,
        adb: Arc<dyn AdbWorker>,
        store: Arc<dyn SnapshotStore>,
        settings: AppSettings,
        db: Option<Db>,
        metadata: HashMap<u32, InstanceMeta>,
    ) -> Self {
        let queue = CommandQueue::new(settings.max_concurrency as usize);
        Self {
            memuc,
            geo,
            adb,
            store,
            queue,
            registry: Mutex::new(Vec::new()),
            settings: Mutex::new(settings),
            metadata: Mutex::new(metadata),
            db,
            pool: Mutex::new(VecDeque::new()),
            create_lock: Mutex::new(()),
            running_sessions: Mutex::new(HashSet::new()),
        }
    }

    /// Ghi write-through xuống SQLite (best-effort; lỗi chỉ log).
    fn persist(&self, index: u32, entry: &InstanceMeta) {
        if let Some(db) = &self.db {
            if let Err(e) = db.upsert(index, entry) {
                tracing::warn!(error = %e, index, "Ghi metadata vào SQLite thất bại");
            }
        }
    }

    /// Gộp metadata (account, last_launched_at, country, note) vào danh sách từ memuc.
    pub async fn merge_metadata(&self, mut instances: Vec<Instance>) -> Vec<Instance> {
        let meta = self.metadata.lock().await;
        for inst in &mut instances {
            if let Some(m) = meta.get(&inst.index) {
                inst.account = m.account.clone();
                inst.last_launched_at = m.last_launched_at;
                inst.country = m.country.clone();
                inst.note = m.note.clone();
            }
        }
        instances
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

    pub async fn set_account(&self, index: u32, account: AccountProfile) {
        let entry = {
            let mut meta = self.metadata.lock().await;
            let e = meta.entry(index).or_default();
            e.account = Some(account);
            e.clone()
        };
        self.persist(index, &entry);
    }

    pub async fn set_hardware(&self, index: u32, hardware: HardwareProfile) {
        let entry = {
            let mut meta = self.metadata.lock().await;
            let e = meta.entry(index).or_default();
            e.hardware = Some(hardware);
            e.clone()
        };
        self.persist(index, &entry);
    }

    /// Lấy fingerprint đã lưu của một VM (để áp lại khi khởi chạy).
    pub async fn hardware_of(&self, index: u32) -> Option<HardwareProfile> {
        self.metadata
            .lock()
            .await
            .get(&index)
            .and_then(|m| m.hardware.clone())
    }

    pub async fn set_note(&self, index: u32, note: String) {
        let entry = {
            let mut meta = self.metadata.lock().await;
            let e = meta.entry(index).or_default();
            e.note = note;
            e.clone()
        };
        self.persist(index, &entry);
    }

    /// Gán country tường minh (người dùng nhập lúc tạo VM). Chuỗi rỗng → None.
    pub async fn set_country(&self, index: u32, country: Option<String>) {
        let country = country
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty());
        let entry = {
            let mut meta = self.metadata.lock().await;
            let e = meta.entry(index).or_default();
            e.country = country;
            e.clone()
        };
        self.persist(index, &entry);
    }

    /// Quốc gia đã lưu của VM (quốc gia yêu cầu, để đối chiếu khi khởi chạy).
    pub async fn country_of(&self, index: u32) -> Option<String> {
        self.metadata
            .lock()
            .await
            .get(&index)
            .and_then(|m| m.country.clone())
    }

    /// Chỉ gán country nếu chưa có; trả về true nếu vừa cập nhật.
    pub async fn set_country_if_empty(&self, index: u32, country: String) -> bool {
        let entry = {
            let mut meta = self.metadata.lock().await;
            let e = meta.entry(index).or_default();
            if e.country.is_some() {
                return false;
            }
            e.country = Some(country);
            e.clone()
        };
        self.persist(index, &entry);
        true
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
