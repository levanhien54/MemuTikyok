//! Command queue limiting heavy VM commands that may run in parallel.
//! The limit can be changed at runtime when Settings are saved.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

#[derive(Clone)]
pub struct CommandQueue {
    inner: Arc<QueueInner>,
}

struct QueueInner {
    limit: AtomicUsize,
    in_flight: AtomicUsize,
    notify: Notify,
}

struct QueuePermit {
    inner: Arc<QueueInner>,
}

impl CommandQueue {
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            inner: Arc::new(QueueInner {
                limit: AtomicUsize::new(max_concurrency.max(1)),
                in_flight: AtomicUsize::new(0),
                notify: Notify::new(),
            }),
        }
    }

    /// Update the queue limit without restarting the app.
    ///
    /// Lowering the limit never cancels running tasks; it only prevents new
    /// tasks from entering until the in-flight count falls below the new limit.
    pub fn set_limit(&self, max_concurrency: usize) {
        self.inner
            .limit
            .store(max_concurrency.max(1), Ordering::Release);
        self.inner.notify.notify_waiters();
    }

    /// Run an async task after a queue slot becomes available.
    pub async fn run<F, T>(&self, task: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let _permit = self.acquire().await;
        task.await
    }

    #[allow(dead_code)]
    pub fn available_permits(&self) -> usize {
        self.inner
            .limit
            .load(Ordering::Acquire)
            .saturating_sub(self.inner.in_flight.load(Ordering::Acquire))
    }

    async fn acquire(&self) -> QueuePermit {
        loop {
            if let Some(permit) = self.try_acquire() {
                return permit;
            }

            let notified = self.inner.notify.notified();
            if let Some(permit) = self.try_acquire() {
                return permit;
            }
            notified.await;
        }
    }

    fn try_acquire(&self) -> Option<QueuePermit> {
        loop {
            let current = self.inner.in_flight.load(Ordering::Acquire);
            let limit = self.inner.limit.load(Ordering::Acquire).max(1);
            if current >= limit {
                return None;
            }

            if self
                .inner
                .in_flight
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(QueuePermit {
                    inner: self.inner.clone(),
                });
            }
        }
    }
}

impl Drop for QueuePermit {
    fn drop(&mut self) {
        self.inner.in_flight.fetch_sub(1, Ordering::AcqRel);
        self.inner.notify.notify_one();
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
        assert!(peak.load(Ordering::SeqCst) <= 2, "vuot gioi han song song");
    }

    #[tokio::test]
    async fn cap_nhat_gioi_han_danh_thuc_task_dang_cho() {
        let queue = CommandQueue::new(1);
        let release_first = Arc::new(Notify::new());
        let entered = Arc::new(AtomicUsize::new(0));

        let q1 = queue.clone();
        let release = release_first.clone();
        let entered_1 = entered.clone();
        let first = tokio::spawn(async move {
            q1.run(async {
                entered_1.fetch_add(1, Ordering::SeqCst);
                release.notified().await;
            })
            .await;
        });

        while entered.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(5)).await;
        }

        let q2 = queue.clone();
        let entered_2 = entered.clone();
        let second = tokio::spawn(async move {
            q2.run(async {
                entered_2.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        });

        sleep(Duration::from_millis(30)).await;
        assert_eq!(
            entered.load(Ordering::SeqCst),
            1,
            "limit 1 thi task thu hai phai doi"
        );

        queue.set_limit(2);
        tokio::time::timeout(Duration::from_secs(1), async {
            while entered.load(Ordering::SeqCst) < 2 {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("tang limit phai danh thuc task dang cho");

        release_first.notify_one();
        first.await.unwrap();
        second.await.unwrap();
    }

    #[tokio::test]
    async fn giam_gioi_han_cho_task_moi_den_khi_duoi_limit() {
        let queue = CommandQueue::new(2);
        let release = Arc::new(Notify::new());
        let entered = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..2 {
            let q = queue.clone();
            let release = release.clone();
            let entered = entered.clone();
            handles.push(tokio::spawn(async move {
                q.run(async {
                    entered.fetch_add(1, Ordering::SeqCst);
                    release.notified().await;
                })
                .await;
            }));
        }

        while entered.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(5)).await;
        }

        queue.set_limit(1);
        let q3 = queue.clone();
        let entered_3 = entered.clone();
        let third = tokio::spawn(async move {
            q3.run(async {
                entered_3.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        });

        sleep(Duration::from_millis(30)).await;
        assert_eq!(
            entered.load(Ordering::SeqCst),
            2,
            "giam limit khong cho task moi vao khi con qua nhieu task dang chay"
        );

        release.notify_one();
        sleep(Duration::from_millis(30)).await;
        assert_eq!(
            entered.load(Ordering::SeqCst),
            2,
            "van dang bang limit moi thi task thu ba phai doi"
        );

        release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            while entered.load(Ordering::SeqCst) < 3 {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("task thu ba phai chay khi in-flight xuong duoi limit");

        for h in handles {
            h.await.unwrap();
        }
        third.await.unwrap();
    }
}
