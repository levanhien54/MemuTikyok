//! Kiểu dữ liệu miền (domain model) — khớp với `src/types/instance.ts` phía FE (§8.5 SRS).

use serde::{Deserialize, Serialize};

/// Package TikTok bản global (ĐÃ XÁC MINH — BACKUP_RESTORE_DESIGN §2).
pub const TIKTOK_PKG: &str = "com.zhiliaoapp.musically";

/// Đường dẫn APK TikTok mặc định (người dùng có thể đổi trong Settings).
pub const DEFAULT_TIKTOK_APK: &str = r"D:\MemuTiktok\appTiktok\tiktok-40-0-0.apk";

/// App thừa gỡ MẶC ĐỊNH khi chuẩn bị VM (đã kiểm chứng an toàn trên MEmu — giữ GMS/GSF).
pub const DEFAULT_BLOAT: &[&str] = &[
    "com.android.gallery3d",
    "com.google.android.play.games",
    "com.google.android.syncadapters.calendar",
    "com.google.android.syncadapters.contacts",
    "com.google.android.apps.pixelmigrate",
    "com.google.android.apps.restore",
    "com.google.android.backuptransport",
    "com.google.android.feedback",
];

/// Hồ sơ phần cứng cố định per-account (§6 thiết kế). Áp y hệt mỗi phiên để
/// giữ nhất quán fingerprint (R-12). android_id set qua adb, còn lại qua memuc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub model: String,
    pub brand: String,
    pub manufacturer: String,
    pub imei: String,
    pub android_id: String,
    pub mac: String,
    pub res_width: u32,
    pub res_height: u32,
    pub dpi: u32,
}

impl HardwareProfile {
    /// Các cặp (key, value) cho `memuc setconfigex` (1 giá trị/khoá).
    /// KHÔNG gồm android_id (adb) và custom_resolution (cần 3 tham số → set_resolution).
    pub fn memuc_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("imei", self.imei.clone()),
            ("microvirt_vm_model", self.model.clone()),
            ("microvirt_vm_manufacturer", self.manufacturer.clone()),
            ("microvirt_vm_brand", self.brand.clone()),
            ("macaddress", self.mac.clone()),
            ("enable_su", "1".to_string()),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstanceStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

/// Hồ sơ tài khoản TikTok gắn với một VM (MPM tự quản; memuc không biết).
/// ⚠️ NHẠY CẢM: chứa mật khẩu/2FA/passkey. KHÔNG log; khi persist phải mã hóa
/// (DPAPI — SEC-3 §9 SRS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub tiktok_username: String,
    pub tiktok_password: String,
    pub two_fa: String,
    pub tiktok_passkey: String,
    pub email: String,
    pub email_password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub index: u32,
    pub title: String,
    pub status: InstanceStatus,
    pub pid: Option<u32>,
    pub window_handle: Option<i64>,
    pub ip: Option<String>,
    pub disk_usage_bytes: Option<u64>,
    /// Thời điểm khởi chạy gần nhất (epoch ms). None nếu chưa từng chạy.
    pub last_launched_at: Option<i64>,
    /// Mã quốc gia ISO 3166-1 alpha-2, nhận theo IP thực khi chạy. None nếu chưa rõ.
    pub country: Option<String>,
    /// Ghi chú tự do của người dùng.
    #[serde(default)]
    pub note: String,
    /// Hồ sơ tài khoản (MPM tự quản, merge từ metadata store).
    pub account: Option<AccountProfile>,
}

/// Payload tạo VM kèm hồ sơ tài khoản.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstancePayload {
    pub account: AccountProfile,
    #[serde(default)]
    pub note: String,
    /// Quốc gia yêu cầu (ISO alpha-2, vd "VN"). Rỗng/None = không ràng buộc khi chạy.
    #[serde(default)]
    pub country: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BulkOperation {
    Start,
    Stop,
    Reboot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub memu_path: Option<String>,
    pub poll_interval_ms: u32,
    pub max_concurrency: u8,
    pub theme: String,
    pub layout: String,
    /// Đường dẫn APK TikTok (None = dùng mặc định DEFAULT_TIKTOK_APK).
    #[serde(default)]
    pub tiktok_apk_path: Option<String>,
    /// Số VM giữ nóng trong warm pool (0 = tắt tự động refill).
    #[serde(default)]
    pub warm_pool_target: u8,
    /// VM base để clone vào pool (None = chưa cấu hình).
    #[serde(default)]
    pub pool_base_index: Option<u32>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            memu_path: None,
            poll_interval_ms: 3000,
            max_concurrency: 3,
            theme: "dark".to_string(),
            layout: "list".to_string(),
            tiktok_apk_path: None,
            warm_pool_target: 0,
            pool_base_index: None,
        }
    }
}

/// Một dấu vết emulator được scan (chẩn đoán chống phát hiện).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorTell {
    pub check: String,
    pub detected: bool,
    pub detail: String,
}

/// Metadata một archive backup (kết quả từ AdbWorker.backup).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMeta {
    pub sha256: String,
    pub size_bytes: u64,
    pub apk_version: String,
}

/// Bản ghi snapshot trong CSDL (dùng cho restore & hiển thị).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRecord {
    pub storage_key: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub apk_version: Option<String>,
    pub created_at: i64,
}

/// Payload cho sự kiện đẩy `instances:update` (§8.4 SRS).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancesUpdateEvent {
    pub instances: Vec<Instance>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_tuong_thich_nguoc_ban_cu() {
        // settings.json bản CŨ (thiếu tiktokApkPath/warmPoolTarget/poolBaseIndex)
        // phải nạp được, không reset về mặc định — nhờ #[serde(default)].
        let old = r#"{"memuPath":"D:/Microvirt/MEmu","pollIntervalMs":3000,
            "maxConcurrency":3,"theme":"dark","layout":"list"}"#;
        let s: AppSettings = serde_json::from_str(old).expect("phải nạp được bản cũ");
        assert_eq!(s.memu_path.as_deref(), Some("D:/Microvirt/MEmu"));
        assert_eq!(s.warm_pool_target, 0);
        assert!(s.tiktok_apk_path.is_none());
        assert!(s.pool_base_index.is_none());
    }

    #[test]
    fn settings_vong_tron_serde() {
        let s = AppSettings {
            memu_path: Some("D:/Microvirt/MEmu/memuc.exe".into()),
            tiktok_apk_path: Some("D:/a.apk".into()),
            warm_pool_target: 3,
            pool_base_index: Some(9),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.warm_pool_target, 3);
        assert_eq!(back.pool_base_index, Some(9));
        assert_eq!(back.tiktok_apk_path.as_deref(), Some("D:/a.apk"));
    }
}
