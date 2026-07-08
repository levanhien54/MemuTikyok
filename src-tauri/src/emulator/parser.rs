use crate::model::{Instance, InstanceStatus};
use serde_json::Value;

/// Parse toàn bộ stdout của `MuMuManager.exe info -v all` (JSON) thành danh sách instance.
pub fn parse_mumu_info(stdout: &str) -> Vec<Instance> {
    let mut instances = Vec::new();
    let root: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Lỗi parse JSON info từ MuMuManager: {}", e);
            return instances;
        }
    };

    if let Some(map) = root.as_object() {
        for (key, val) in map {
            let index = match key.parse::<u32>() {
                Ok(i) => i,
                Err(_) => continue,
            };

            let title = val["name"].as_str().unwrap_or("").to_string();
            let is_started = val["is_android_started"].as_bool().unwrap_or(false);
            let status = if is_started {
                InstanceStatus::Running
            } else {
                InstanceStatus::Stopped
            };
            let disk_usage_bytes = val["disk_size_bytes"].as_u64();

            instances.push(Instance {
                index,
                title,
                status,
                window_handle: None,
                pid: None,
                disk_usage_bytes,
                ip: None,
                last_launched_at: None,
                country: None,
                note: String::new(),
                account: None,
            });
        }
    }

    instances
}
