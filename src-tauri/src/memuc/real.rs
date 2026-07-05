//! Adapter thật gọi `memuc.exe`.
//!
//! An toàn (§9 SRS):
//! - **KHÔNG** dùng shell; mọi tham số truyền qua `arg()` (chống injection — SEC-1).
//! - Mọi lệnh có **timeout** (NFR-R2); hết giờ thì kill child process.
//! - Tên VM do người dùng nhập được **validate whitelist** trước khi dùng.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::time::timeout;

use super::parser::parse_listvms;
use super::MemucClient;
use crate::error::{AppError, AppResult};
use crate::model::Instance;

/// Timeout cho lệnh nhanh (query/config): listvms, setconfigex, rename…
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout cho lệnh VÒNG ĐỜI VM (create/clone/start/stop/reboot/remove) — các thao
/// tác này nạp/tắt máy ảo, có thể vượt xa 15s khi host tải nặng (nhiều VM chạy) →
/// dùng ngưỡng rộng để tránh Timeout GIẢ làm hỏng luồng khởi chạy. (Phát hiện qua
/// test thực: provision bị Timeout(15) khi start VM lúc host đang chạy VM khác.)
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct RealMemuc {
    memuc_path: PathBuf,
    timeout: Duration,
}

impl RealMemuc {
    pub fn new(memuc_path: impl Into<PathBuf>) -> Self {
        Self {
            memuc_path: memuc_path.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Dò đường dẫn memuc.exe ở các vị trí cài đặt phổ biến trên nhiều ổ đĩa
    /// (FR-E-3 / R-03). MEmu có thể cài ngoài Program Files, kể cả ổ D:/E:
    /// (đã gặp thực tế: `D:\Microvirt\MEmu`). Bản đầy đủ nên đọc thêm Registry.
    pub fn discover() -> Option<PathBuf> {
        const SUBDIRS: &[&str] = &[
            r"Program Files\Microvirt\MEmu",
            r"Program Files (x86)\Microvirt\MEmu",
            r"Microvirt\MEmu",
        ];
        for drive in ['C', 'D', 'E', 'F'] {
            for sub in SUBDIRS {
                let candidate = PathBuf::from(format!(r"{drive}:\{sub}\memuc.exe"));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Chạy memuc với danh sách đối số (không qua shell), dùng timeout mặc định.
    async fn run(&self, args: &[&str]) -> AppResult<String> {
        self.run_to(args, self.timeout).await
    }

    /// Như `run` nhưng cho phép chỉ định timeout (lệnh vòng đời VM cần ngưỡng rộng hơn).
    async fn run_to(&self, args: &[&str], dur: Duration) -> AppResult<String> {
        let mut cmd = Command::new(&self.memuc_path);
        cmd.args(args);
        // Hết giờ (timeout) thì HỦY tiến trình con thật sự — nếu không, tokio chỉ drop
        // future và memuc.exe con vẫn chạy ngầm (mồ côi, có thể hoàn tất thao tác lệch trạng thái).
        cmd.kill_on_drop(true);
        // Ẩn cửa sổ console con trên Windows (tokio::process::Command có sẵn creation_flags).
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let output = timeout(dur, cmd.output())
            .await
            .map_err(|_| AppError::Timeout(dur.as_secs()))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::CommandFailed(if stderr.is_empty() {
                format!("exit code {:?}", output.status.code())
            } else {
                stderr
            }));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Whitelist tên VM: chữ, số, khoảng trắng, `-`, `_`, `.` (SEC-1 / FR-B-5).
fn validate_title(title: &str) -> AppResult<()> {
    let t = title.trim();
    if t.is_empty() {
        return Err(AppError::InvalidInput("Tên máy ảo không được rỗng".into()));
    }
    if t.len() > 64 {
        return Err(AppError::InvalidInput(
            "Tên máy ảo quá dài (tối đa 64 ký tự)".into(),
        ));
    }
    if !t
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
    {
        return Err(AppError::InvalidInput("Tên chứa ký tự không hợp lệ".into()));
    }
    Ok(())
}

#[async_trait]
impl MemucClient for RealMemuc {
    async fn list_instances(&self) -> AppResult<Vec<Instance>> {
        let stdout = self.run(&["listvms"]).await?;
        Ok(parse_listvms(&stdout))
    }

    async fn start(&self, index: u32) -> AppResult<()> {
        self.run_to(&["start", "-i", &index.to_string()], LIFECYCLE_TIMEOUT)
            .await
            .map(|_| ())
    }

    async fn stop(&self, index: u32) -> AppResult<()> {
        self.run_to(&["stop", "-i", &index.to_string()], LIFECYCLE_TIMEOUT)
            .await
            .map(|_| ())
    }

    async fn reboot(&self, index: u32) -> AppResult<()> {
        self.run_to(&["reboot", "-i", &index.to_string()], LIFECYCLE_TIMEOUT)
            .await
            .map(|_| ())
    }

    async fn create(&self) -> AppResult<()> {
        self.run_to(&["create"], LIFECYCLE_TIMEOUT).await.map(|_| ())
    }

    async fn clone_vm(&self, index: u32) -> AppResult<()> {
        self.run_to(&["clone", "-i", &index.to_string()], LIFECYCLE_TIMEOUT)
            .await
            .map(|_| ())
    }

    async fn remove(&self, index: u32) -> AppResult<()> {
        self.run_to(&["remove", "-i", &index.to_string()], LIFECYCLE_TIMEOUT)
            .await
            .map(|_| ())
    }

    async fn rename(&self, index: u32, title: &str) -> AppResult<()> {
        validate_title(title)?;
        self.run(&["rename", "-i", &index.to_string(), title])
            .await
            .map(|_| ())
    }

    async fn set_config(&self, index: u32, key: &str, value: &str) -> AppResult<()> {
        self.run(&["setconfigex", "-i", &index.to_string(), key, value])
            .await
            .map(|_| ())
    }

    async fn set_resolution(&self, index: u32, width: u32, height: u32, dpi: u32) -> AppResult<()> {
        // memuc: setconfigex -i N custom_resolution <w> <h> <dpi> (3 tham số riêng).
        self.run(&[
            "setconfigex",
            "-i",
            &index.to_string(),
            "custom_resolution",
            &width.to_string(),
            &height.to_string(),
            &dpi.to_string(),
        ])
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tu_choi_ten_rong() {
        assert!(validate_title("").is_err());
        assert!(validate_title("   ").is_err());
    }

    #[test]
    fn tu_choi_ky_tu_nguy_hiem() {
        // Ngăn injection: ký tự shell/đường dẫn bị chặn.
        assert!(validate_title("vm & del").is_err());
        assert!(validate_title("a\"b").is_err());
        assert!(validate_title("a|b").is_err());
        assert!(validate_title("../evil").is_err());
    }

    #[test]
    fn chap_nhan_ten_hop_le() {
        assert!(validate_title("MEmu-01").is_ok());
        assert!(validate_title("Tester A.2").is_ok());
        assert!(validate_title("farm_09").is_ok());
    }
}
