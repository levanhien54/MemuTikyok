//! Bắt log ứng dụng vào một ring buffer để LogsView hiển thị (chẩn đoán trong app).
//!
//! App dùng `tracing::warn!/info!` nhưng TRƯỚC bản này KHÔNG có subscriber nào → mọi
//! log của app bị rơi (chỉ Tauri nội bộ log qua plugin). Module này cài một subscriber
//! `fmt` ghi vào (a) stderr (console dev) và (b) một VecDeque giới hạn (LogsView đọc qua
//! lệnh `get_logs`). Nhờ đó các warn quan trọng (provision lỗi, install retry, reconcile
//! hủy VM mồ côi, backup fail…) hiện được cho người vận hành.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

/// Buffer log dùng chung (managed bởi Tauri → lệnh `get_logs` đọc).
pub type LogBuffer = Arc<Mutex<VecDeque<String>>>;

/// Giữ tối đa bấy nhiêu dòng log gần nhất.
const CAP: usize = 800;

struct RingWriter(LogBuffer);

impl Write for RingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let s = String::from_utf8_lossy(buf);
        if let Ok(mut b) = self.0.lock() {
            for line in s.lines() {
                let t = line.trim_end();
                if !t.is_empty() {
                    b.push_back(t.to_string());
                    while b.len() > CAP {
                        b.pop_front();
                    }
                }
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct RingMakeWriter(LogBuffer);

impl<'a> MakeWriter<'a> for RingMakeWriter {
    type Writer = RingWriter;
    fn make_writer(&'a self) -> Self::Writer {
        RingWriter(self.0.clone())
    }
}

/// Cài subscriber tracing (một lần, lúc khởi động) và trả buffer để manage + đọc.
/// An toàn khi gọi trùng: dùng `try_init` nên không panic nếu đã có global subscriber.
pub fn init() -> LogBuffer {
    let buffer: LogBuffer = Arc::new(Mutex::new(VecDeque::new()));
    // Chỉ bắt INFO trở lên (bỏ trace/debug ồn). Ghi ring (LogsView) + stderr (dev console).
    let filter = tracing_subscriber::filter::LevelFilter::INFO;
    let ring = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(RingMakeWriter(buffer.clone()));
    let console = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(ring)
        .with(console)
        .try_init();
    buffer
}
