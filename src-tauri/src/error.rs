//! Phân loại lỗi tập trung (§11 SRS). Mỗi biến thể ánh xạ một thông điệp
//! rõ ràng cho người dùng; serialize để trả về frontend qua Tauri command.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Không tìm thấy MuMuManager.exe. Hãy trỏ đường dẫn MuMu trong Settings.")]
    EmulatorNotFound,

    #[error("Lệnh emulator hết thời gian chờ ({0}s).")]
    Timeout(u64),

    #[error("emulator trả về lỗi: {0}")]
    CommandFailed(String),

    // Dành cho parser khi cần báo lỗi cứng; hiện parser fault-tolerant nên chưa dùng.
    #[allow(dead_code)]
    #[error("Không đọc được dữ liệu từ emulator: {0}")]
    ParseError(String),

    #[error("Tham số không hợp lệ: {0}")]
    InvalidInput(String),

    #[error("Quốc gia IP thoát ({actual}) không khớp quốc gia yêu cầu ({expected}). Không khởi chạy để tránh sai lệch định vị.")]
    CountryMismatch { actual: String, expected: String },

    #[error("Không xác thực được quốc gia IP thoát để đối chiếu với '{0}'. Kiểm tra mạng/proxy rồi thử lại.")]
    CountryUnverified(String),

    #[error("Lỗi vào/ra: {0}")]
    Io(String),

    #[error("Lỗi cơ sở dữ liệu: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Database(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

/// Tauri yêu cầu error type serialize được để trả về JS.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
