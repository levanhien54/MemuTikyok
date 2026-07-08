//! Lớp tích hợp MuMu Player. Toàn bộ tương tác với `MuMuManager.exe` đi qua trait
//! [`EmulatorClient`] (§7, §8 SRS). Nhờ trừu tượng hóa này ta test được bằng
//! [`mock::MockClient`] mà không cần MuMu thật (NFR-M1).

mod mock;
mod mumu;
mod parser;

pub use mock::MockClient;
pub use mumu::MumuClient;

use crate::error::AppResult;
use crate::model::Instance;
use async_trait::async_trait;

#[async_trait]
pub trait EmulatorClient: Send + Sync {
    /// Liệt kê toàn bộ VM (nguồn sự thật của trạng thái).
    async fn list_instances(&self) -> AppResult<Vec<Instance>>;

    async fn start(&self, index: u32) -> AppResult<()>;
    async fn stop(&self, index: u32) -> AppResult<()>;
    async fn create(&self) -> AppResult<()>;
    async fn remove(&self, index: u32) -> AppResult<()>;

    /// Áp một cấu hình.
    async fn set_config(&self, index: u32, key: &str, value: &str) -> AppResult<()>;

    /// Đặt độ phân giải + DPI.
    async fn set_resolution(&self, index: u32, width: u32, height: u32, dpi: u32) -> AppResult<()>;
}
