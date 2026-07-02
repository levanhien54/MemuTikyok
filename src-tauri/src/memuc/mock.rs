//! Adapter giả lập trong bộ nhớ — dùng cho test và chạy dev khi không có MEmu.
//! Mô phỏng hành vi bất đồng bộ (§7.2): thao tác đổi trạng thái ngay trong state.

use std::collections::HashSet;
use std::sync::Mutex;

use async_trait::async_trait;

use super::MemucClient;
use crate::error::{AppError, AppResult};
use crate::model::{Instance, InstanceStatus};

/// Index rảnh nhỏ nhất (mô phỏng memuc **tái dùng** index đã xóa để lấp khoảng trống).
fn lowest_free_index(vms: &[Instance]) -> u32 {
    let used: HashSet<u32> = vms.iter().map(|v| v.index).collect();
    (0u32..).find(|i| !used.contains(i)).unwrap_or(0)
}

pub struct MockMemuc {
    state: Mutex<Vec<Instance>>,
    configs: Mutex<Vec<(u32, String, String)>>,
}

impl MockMemuc {
    /// Lấy giá trị config đã set gần nhất (dùng trong test).
    #[cfg(test)]
    pub fn config_value(&self, index: u32, key: &str) -> Option<String> {
        self.configs
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(i, k, _)| *i == index && k == key)
            .map(|(_, _, v)| v.clone())
    }

    pub fn new() -> Self {
        Self {
            configs: Mutex::new(Vec::new()),
            state: Mutex::new(vec![
                Instance {
                    index: 0,
                    title: "MEmu".into(),
                    status: InstanceStatus::Running,
                    pid: Some(10508),
                    window_handle: Some(197666),
                    ip: Some("192.168.1.20".into()),
                    disk_usage_bytes: Some(4096),
                    last_launched_at: None,
                    country: None,
                    note: String::new(),
                    account: None,
                },
                Instance {
                    index: 1,
                    title: "MEmu_1".into(),
                    status: InstanceStatus::Stopped,
                    pid: None,
                    window_handle: None,
                    ip: None,
                    disk_usage_bytes: Some(2048),
                    last_launched_at: None,
                    country: None,
                    note: String::new(),
                    account: None,
                },
            ]),
        }
    }

    fn set_status(&self, index: u32, status: InstanceStatus) -> AppResult<()> {
        let mut vms = self.state.lock().unwrap();
        let vm = vms
            .iter_mut()
            .find(|v| v.index == index)
            .ok_or_else(|| AppError::InvalidInput(format!("Không có VM index {index}")))?;
        vm.status = status;
        Ok(())
    }
}

impl Default for MockMemuc {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemucClient for MockMemuc {
    async fn list_instances(&self) -> AppResult<Vec<Instance>> {
        Ok(self.state.lock().unwrap().clone())
    }

    async fn start(&self, index: u32) -> AppResult<()> {
        self.set_status(index, InstanceStatus::Running)
    }

    async fn stop(&self, index: u32) -> AppResult<()> {
        self.set_status(index, InstanceStatus::Stopped)
    }

    async fn reboot(&self, index: u32) -> AppResult<()> {
        self.set_status(index, InstanceStatus::Running)
    }

    async fn create(&self) -> AppResult<()> {
        let mut vms = self.state.lock().unwrap();
        let next = lowest_free_index(&vms);
        vms.push(Instance {
            index: next,
            title: format!("MEmu_{next}"),
            status: InstanceStatus::Stopped,
            pid: None,
            window_handle: None,
            ip: None,
            disk_usage_bytes: Some(2048),
            last_launched_at: None,
            country: None,
            note: String::new(),
            account: None,
        });
        Ok(())
    }

    async fn clone_vm(&self, index: u32) -> AppResult<()> {
        let mut vms = self.state.lock().unwrap();
        let src_title = vms
            .iter()
            .find(|v| v.index == index)
            .map(|v| v.title.clone())
            .unwrap_or_else(|| "MEmu".to_string());
        let next = lowest_free_index(&vms);
        vms.push(Instance {
            index: next,
            title: format!("{src_title}_clone"),
            status: InstanceStatus::Stopped,
            pid: None,
            window_handle: None,
            ip: None,
            disk_usage_bytes: Some(2048),
            last_launched_at: None,
            country: None,
            note: String::new(),
            account: None,
        });
        Ok(())
    }

    async fn remove(&self, index: u32) -> AppResult<()> {
        self.state.lock().unwrap().retain(|v| v.index != index);
        Ok(())
    }

    async fn rename(&self, index: u32, title: &str) -> AppResult<()> {
        let mut vms = self.state.lock().unwrap();
        let vm = vms
            .iter_mut()
            .find(|v| v.index == index)
            .ok_or_else(|| AppError::InvalidInput(format!("Không có VM index {index}")))?;
        vm.title = title.to_string();
        Ok(())
    }

    async fn set_config(&self, index: u32, key: &str, value: &str) -> AppResult<()> {
        self.configs
            .lock()
            .unwrap()
            .push((index, key.to_string(), value.to_string()));
        Ok(())
    }

    async fn set_resolution(&self, index: u32, width: u32, height: u32, dpi: u32) -> AppResult<()> {
        self.configs.lock().unwrap().push((
            index,
            "custom_resolution".to_string(),
            format!("{width} {height} {dpi}"),
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_stop_doi_trang_thai() {
        let m = MockMemuc::new();
        m.stop(0).await.unwrap();
        let vms = m.list_instances().await.unwrap();
        assert_eq!(vms[0].status, InstanceStatus::Stopped);

        m.start(0).await.unwrap();
        let vms = m.list_instances().await.unwrap();
        assert_eq!(vms[0].status, InstanceStatus::Running);
    }

    #[tokio::test]
    async fn create_va_remove() {
        let m = MockMemuc::new();
        m.create().await.unwrap();
        assert_eq!(m.list_instances().await.unwrap().len(), 3);
        m.remove(2).await.unwrap();
        assert_eq!(m.list_instances().await.unwrap().len(), 2);
    }
}
