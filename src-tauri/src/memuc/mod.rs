//! Lớp tích hợp MEmu. Toàn bộ tương tác với `memuc.exe`/`adb.exe` đi qua trait
//! [`MemucClient`] (§7, §8 SRS). Nhờ trừu tượng hóa này ta test được bằng
//! [`mock::MockMemuc`] mà không cần MEmu thật (NFR-M1) và cô lập rủi ro đổi
//! format của MEmu (R-07).

mod mock;
mod parser;
mod real;

pub use mock::MockMemuc;
pub use real::RealMemuc;

use crate::error::AppResult;
use crate::model::Instance;
use async_trait::async_trait;

#[async_trait]
pub trait MemucClient: Send + Sync {
    /// Liệt kê toàn bộ VM (nguồn sự thật của trạng thái — §7.2).
    async fn list_instances(&self) -> AppResult<Vec<Instance>>;

    async fn start(&self, index: u32) -> AppResult<()>;
    async fn stop(&self, index: u32) -> AppResult<()>;
    async fn create(&self) -> AppResult<()>;
    async fn remove(&self, index: u32) -> AppResult<()>;

    /// Áp một cấu hình qua `memuc setconfigex` (vd imei, model).
    async fn set_config(&self, index: u32, key: &str, value: &str) -> AppResult<()>;

    /// Đặt độ phân giải + DPI (cửa sổ VM khớp thiết bị fake). memuc cần 3 tham số riêng.
    async fn set_resolution(&self, index: u32, width: u32, height: u32, dpi: u32) -> AppResult<()>;
}
