use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::EmulatorClient;
use crate::error::AppResult;
use crate::model::{Instance, InstanceStatus};

/// Mock để test logic không cần MuMu thật (chạy độc lập frontend).
/// Giả lập độ trễ IO và trạng thái stateful.
#[derive(Clone)]
pub struct MockClient {
    state: Arc<Mutex<MockState>>,
}

struct MockState {
    instances: HashMap<u32, Instance>,
    next_id: u32,
    configs: HashMap<(u32, String), String>,
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClient {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                instances: HashMap::new(),
                next_id: 1, // index thường bắt đầu từ 0, nhưng ta để 1 làm ví dụ.
                configs: HashMap::new(),
            })),
        }
    }
}

#[async_trait::async_trait]
impl EmulatorClient for MockClient {
    async fn list_instances(&self) -> AppResult<Vec<Instance>> {
        sleep(Duration::from_millis(100)).await;
        let state = self.state.lock().await;
        let mut list: Vec<_> = state.instances.values().cloned().collect();
        list.sort_by_key(|i| i.index);
        Ok(list)
    }

    async fn start(&self, index: u32) -> AppResult<()> {
        sleep(Duration::from_millis(500)).await;
        let mut state = self.state.lock().await;
        if let Some(vm) = state.instances.get_mut(&index) {
            vm.status = InstanceStatus::Running;
            // pid giả
            vm.pid = Some(12345);
        }
        Ok(())
    }

    async fn stop(&self, index: u32) -> AppResult<()> {
        sleep(Duration::from_millis(500)).await;
        let mut state = self.state.lock().await;
        if let Some(vm) = state.instances.get_mut(&index) {
            vm.status = InstanceStatus::Stopped;
            vm.pid = None;
        }
        Ok(())
    }

    async fn create(&self) -> AppResult<()> {
        sleep(Duration::from_millis(1000)).await;
        let mut state = self.state.lock().await;

        let index = state.next_id;
        state.next_id += 1;

        let new_vm = Instance {
            index,
            title: format!("MuMu_{}", index),
            status: InstanceStatus::Stopped,
            pid: None,
            window_handle: None,
            disk_usage_bytes: None,
            ip: None,
            last_launched_at: None,
            country: None,
            note: String::new(),
            account: None,
        };
        state.instances.insert(index, new_vm);
        Ok(())
    }

    async fn remove(&self, index: u32) -> AppResult<()> {
        sleep(Duration::from_millis(800)).await;
        let mut state = self.state.lock().await;
        state.instances.remove(&index);
        Ok(())
    }

    async fn set_config(&self, index: u32, key: &str, value: &str) -> AppResult<()> {
        sleep(Duration::from_millis(50)).await;
        let mut state = self.state.lock().await;
        state
            .configs
            .insert((index, key.to_string()), value.to_string());
        Ok(())
    }

    async fn set_resolution(
        &self,
        _index: u32,
        _width: u32,
        _height: u32,
        _dpi: u32,
    ) -> AppResult<()> {
        sleep(Duration::from_millis(50)).await;
        Ok(())
    }
}

#[cfg(test)]
impl MockClient {
    pub async fn config_value(&self, index: u32, key: &str) -> Option<String> {
        let state = self.state.lock().await;
        state.configs.get(&(index, key.to_string())).cloned()
    }
}
