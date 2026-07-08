use crate::error::{AppError, AppResult};
use crate::model::{Instance, InstanceStatus};
use serde_json::Value;

/// Parse stdout cua `MuMuManager.exe info -v all` thanh danh sach instance.
pub fn parse_mumu_info(stdout: &str) -> AppResult<Vec<Instance>> {
    let root: Value =
        serde_json::from_str(stdout).map_err(|e| AppError::ParseError(e.to_string()))?;
    let mut instances = Vec::new();

    match &root {
        Value::Object(map) if map.contains_key("index") || map.contains_key("vm_index") => {
            if let Some(instance) = parse_instance(None, &root) {
                instances.push(instance);
            }
        }
        Value::Object(map) => {
            for (key, val) in map {
                if let Ok(index) = key.parse::<u32>() {
                    if let Some(instance) = parse_instance(Some(index), val) {
                        instances.push(instance);
                    }
                }
            }
        }
        Value::Array(items) => {
            for val in items {
                if let Some(instance) = parse_instance(None, val) {
                    instances.push(instance);
                }
            }
        }
        _ => {}
    }

    instances.sort_by_key(|i| i.index);
    Ok(instances)
}

fn parse_instance(index_hint: Option<u32>, val: &Value) -> Option<Instance> {
    let index = index_hint
        .or_else(|| val.get("index").and_then(Value::as_u64).map(|v| v as u32))
        .or_else(|| {
            val.get("vm_index")
                .and_then(Value::as_u64)
                .map(|v| v as u32)
        })?;
    let title = val["name"].as_str().unwrap_or("").to_string();
    let is_started = val["is_android_started"].as_bool().unwrap_or(false);
    let status = if is_started {
        InstanceStatus::Running
    } else {
        InstanceStatus::Stopped
    };
    let disk_usage_bytes = val["disk_size_bytes"].as_u64();

    Some(Instance {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keyed_object() {
        let got = parse_mumu_info(
            r#"{"2":{"name":"mpm","is_android_started":true,"disk_size_bytes":42}}"#,
        )
        .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].index, 2);
        assert_eq!(got[0].status, InstanceStatus::Running);
        assert_eq!(got[0].disk_usage_bytes, Some(42));
    }

    #[test]
    fn parse_flat_object() {
        let got =
            parse_mumu_info(r#"{"index":7,"name":"one","is_android_started":false}"#).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].index, 7);
        assert_eq!(got[0].status, InstanceStatus::Stopped);
    }

    #[test]
    fn parse_invalid_json_is_error() {
        assert!(parse_mumu_info("not-json").is_err());
    }
}
