//! Adapter thật gọi `MuMuManager.exe`.
//!
//! An toàn (§9 SRS):
//! - **KHÔNG** dùng shell; mọi tham số truyền qua `arg()` (chống injection — SEC-1).
//! - Mọi lệnh có **timeout** (NFR-R2); hết giờ thì kill child process.
//! - Tên VM do người dùng nhập được **validate whitelist** trước khi dùng.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;
use tokio::time::{sleep, timeout};

use super::parser::parse_mumu_info;
use super::EmulatorClient;
use crate::error::{AppError, AppResult};
use crate::model::Instance;

/// Timeout cho lệnh nhanh (query/config).
#[cfg_attr(feature = "mock-emulator", allow(dead_code))]
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout cho lệnh VÒNG ĐỜI VM (create/clone/start/stop/reboot/remove).
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct MumuClient {
    manager_path: PathBuf,
    timeout: Duration,
}

impl MumuClient {
    #[cfg_attr(feature = "mock-emulator", allow(dead_code))]
    pub fn new(manager_path: impl Into<PathBuf>) -> Self {
        Self {
            manager_path: manager_path.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Dò đường dẫn MuMuManager.exe ở các vị trí cài đặt phổ biến.
    pub fn discover() -> Option<PathBuf> {
        const SUBDIRS: &[&str] = &[
            r"Program Files\Netease\MuMuPlayer-12.0\shell",
            r"Program Files (x86)\Netease\MuMuPlayer-12.0\shell",
            r"Netease\MuMuPlayer-12.0\shell",
            r"Program Files\Microvirt\MuMuPlayer-12.0\shell",
            r"Program Files (x86)\Microvirt\MuMuPlayer-12.0\shell",
            r"Microvirt\MuMuPlayer-12.0\shell",
            r"Program Files\Netease\MuMuPlayer\nx_main",
            r"Program Files (x86)\Netease\MuMuPlayer\nx_main",
            r"Netease\MuMuPlayer\nx_main",
            r"Program Files\Microvirt\MuMuPlayer\nx_main",
            r"Program Files (x86)\Microvirt\MuMuPlayer\nx_main",
            r"Microvirt\MuMuPlayer\nx_main",
            r"Microvirt\MuMu",
            r"Program Files\Microvirt\MuMu",
            r"Program Files (x86)\Microvirt\MuMu",
        ];
        for drive in 'A'..='Z' {
            if !PathBuf::from(format!(r"{drive}:\")).exists() {
                continue;
            }
            for sub in SUBDIRS {
                let candidate = PathBuf::from(format!(r"{drive}:\{sub}\MuMuManager.exe"));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Chạy MuMuManager với danh sách đối số, dùng timeout mặc định.
    async fn run(&self, args: &[&str]) -> AppResult<String> {
        self.run_to(args, self.timeout).await
    }

    /// Như `run` nhưng cho phép chỉ định timeout.
    async fn run_to(&self, args: &[&str], dur: Duration) -> AppResult<String> {
        let mut cmd = Command::new(&self.manager_path);
        cmd.args(args);
        cmd.kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let output = timeout(dur, cmd.output())
            .await
            .map_err(|_| AppError::Timeout(dur.as_secs()))?
            .map_err(command_error)?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::CommandFailed(if stderr.is_empty() {
                if stdout.is_empty() {
                    format!("exit code {:?}", output.status.code())
                } else {
                    stdout
                }
            } else {
                stderr
            }));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if let Some(msg) = mumu_stdout_error(&stdout) {
            return Err(AppError::CommandFailed(msg));
        }
        Ok(stdout)
    }
}

fn mumu_stdout_error(stdout: &str) -> Option<String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(json) = serde_json::from_str::<Value>(trimmed) {
        return has_negative_errcode(&json).then(|| trimmed.to_string());
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("\"errcode\":-")
        || lower.contains("\"errcode\": -")
        || lower.contains("errcode:-")
        || lower.contains("errcode=-")
        || first_stdout_token_is_error(&lower)
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn has_negative_errcode(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let key = key.to_ascii_lowercase();
                if matches!(key.as_str(), "errcode" | "err_code" | "error_code")
                    && value.as_i64().is_some_and(|code| code < 0)
                {
                    return true;
                }
                if has_negative_errcode(value) {
                    return true;
                }
            }
            false
        }
        Value::Array(items) => items.iter().any(has_negative_errcode),
        _ => false,
    }
}

fn first_stdout_token_is_error(lower: &str) -> bool {
    let Some(line) = lower.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return false;
    };
    let token = line
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_ascii_alphanumeric());
    matches!(token, "error" | "failed" | "fail")
}

fn command_error(e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::NotFound {
        AppError::EmulatorNotFound
    } else {
        AppError::Io(e.to_string())
    }
}

fn simulation_config(key: &str, value: &str) -> Option<(String, String)> {
    match key {
        "macaddress" | "mac_address" => Some(("mac_address".into(), value.to_string())),
        "imei" => Some(("imei".into(), value.to_string())),
        "model" => Some(("microvirt_vm_model".into(), value.to_string())),
        "brand" => Some(("microvirt_vm_brand".into(), value.to_string())),
        "manufacturer" => Some(("microvirt_vm_manufacturer".into(), value.to_string())),
        "enable_su" | "root_permission" => {
            let enabled = value == "1" || value.eq_ignore_ascii_case("true");
            Some((
                "enable_su".into(),
                if enabled { "1" } else { "0" }.to_string(),
            ))
        }
        "android_id" => None,
        _ => Some((key.to_string(), value.to_string())),
    }
}

#[async_trait]
impl EmulatorClient for MumuClient {
    async fn list_instances(&self) -> AppResult<Vec<Instance>> {
        let stdout = self.run(&["info", "-v", "all"]).await?;
        parse_mumu_info(&stdout)
    }

    async fn start(&self, index: u32) -> AppResult<()> {
        self.run_to(
            &["control", "-v", &index.to_string(), "launch"],
            LIFECYCLE_TIMEOUT,
        )
        .await
        .map(|_| ())
    }

    async fn stop(&self, index: u32) -> AppResult<()> {
        self.run_to(
            &["control", "-v", &index.to_string(), "shutdown"],
            LIFECYCLE_TIMEOUT,
        )
        .await
        .map(|_| ())
    }

    async fn create(&self) -> AppResult<()> {
        self.run_to(&["clone", "-v", "0"], LIFECYCLE_TIMEOUT)
            .await
            .map(|_| ())
    }

    async fn remove(&self, index: u32) -> AppResult<()> {
        let idx = index.to_string();
        match self
            .run_to(&["delete", "-v", &idx], LIFECYCLE_TIMEOUT)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("-103") && !msg.contains("running") {
                    return Err(e);
                }
                let _ = self
                    .run_to(&["control", "-v", &idx, "shutdown"], LIFECYCLE_TIMEOUT)
                    .await;
                sleep(Duration::from_secs(8)).await;
                self.run_to(&["delete", "-v", &idx], LIFECYCLE_TIMEOUT)
                    .await
                    .map(|_| ())
            }
        }
    }

    async fn set_config(&self, index: u32, key: &str, value: &str) -> AppResult<()> {
        let Some((actual_key, actual_value)) = simulation_config(key, value) else {
            return Ok(());
        };

        self.run(&[
            "simulation",
            "-v",
            &index.to_string(),
            "-sk",
            &actual_key,
            "-sv",
            &actual_value,
        ])
        .await
        .map(|_| ())
    }

    async fn set_resolution(&self, index: u32, width: u32, height: u32, dpi: u32) -> AppResult<()> {
        self.run(&[
            "simulation",
            "-v",
            &index.to_string(),
            "-sk",
            "custom_resolution",
            "-sv",
            &format!("{width},{height},{dpi}"),
        ])
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{mumu_stdout_error, simulation_config};

    #[test]
    fn maps_hardware_keys_to_mumu_simulation_keys() {
        assert_eq!(
            simulation_config("model", "FRD-L19"),
            Some(("microvirt_vm_model".to_string(), "FRD-L19".to_string()))
        );
        assert_eq!(
            simulation_config("brand", "HUAWEI"),
            Some(("microvirt_vm_brand".to_string(), "HUAWEI".to_string()))
        );
        assert_eq!(
            simulation_config("manufacturer", "HUAWEI"),
            Some((
                "microvirt_vm_manufacturer".to_string(),
                "HUAWEI".to_string()
            ))
        );
        assert_eq!(
            simulation_config("mac_address", "02:00:00:11:22:33"),
            Some(("mac_address".to_string(), "02:00:00:11:22:33".to_string()))
        );
    }

    #[test]
    fn normalizes_root_flag_for_simulation() {
        assert_eq!(
            simulation_config("enable_su", "true"),
            Some(("enable_su".to_string(), "1".to_string()))
        );
        assert_eq!(
            simulation_config("root_permission", "false"),
            Some(("enable_su".to_string(), "0".to_string()))
        );
        assert_eq!(simulation_config("android_id", "abc"), None);
    }

    #[test]
    fn stdout_error_ignores_vm_names_in_info_json() {
        let stdout = r#"{"1":{"name":"test-fail-Error99","is_android_started":true}}"#;
        assert_eq!(mumu_stdout_error(stdout), None);
    }

    #[test]
    fn stdout_error_catches_structured_negative_errcode() {
        let stdout = r#"{"errcode":-103,"message":"running"}"#;
        assert_eq!(mumu_stdout_error(stdout), Some(stdout.to_string()));
    }

    #[test]
    fn stdout_error_catches_error_token_at_start_only() {
        assert!(mumu_stdout_error("failed: delete vm").is_some());
        assert_eq!(mumu_stdout_error("vm name contains failed later"), None);
    }
}
