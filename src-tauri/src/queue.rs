//! Command Queue giới hạn số lệnh nặng chạy song song (§8.3 SRS).
//! Chống làm treo host khi bulk-start nhiều VM (R-01, FR-C-2).

use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct CommandQueue {
    semaphore: Arc<Semaphore>,
}

impl CommandQueue {
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency.max(1))),
        }
    }

    /// Chạy một tác vụ async, chỉ khi có "slot" rảnh trong giới hạn song song.
    /// Các lệnh vượt giới hạn sẽ xếp hàng chờ tự động.
    pub async fn run<F, T>(&self, task: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        // Permit được giữ tới hết tác vụ; unwrap an toàn vì semaphore không đóng.
        let _permit = self.semaphore.acquire().await.expect("semaphore closed");
        task.await
    }

    /// Số slot còn rảnh — dành cho chỉ báo tải trên UI (nâng cấp tương lai).
    #[allow(dead_code)]
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn khong_vuot_qua_gioi_han_song_song() {
        let queue = CommandQueue::new(2);
        let concurrent = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..6 {
            let q = queue.clone();
            let concurrent = concurrent.clone();
            let peak = peak.clone();
            handles.push(tokio::spawn(async move {
                q.run(async {
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    sleep(Duration::from_millis(20)).await;
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                })
                .await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert!(peak.load(Ordering::SeqCst) <= 2, "vượt giới hạn song song");
    }
}
