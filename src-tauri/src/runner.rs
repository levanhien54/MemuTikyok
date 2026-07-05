//! Automation runner: chạy phiên "xem feed" TikTok **GIẢ NGƯỜI** (warm-up an toàn —
//! đúng hành vi quan trọng nhất cho độ an toàn account theo nghiên cứu).
//!
//! Dùng lớp input giả-người ([`crate::humanize`]) + thời lượng xem/like/nghỉ ngẫu nhiên.
//! **KHÔNG cần tọa độ UI mong manh:** swipe GIỮA màn hình = cuộn video; **double-tap
//! GIỮA màn hình = like** (TikTok hỗ trợ) → độc lập độ phân giải & bố cục UI.
//!
//! Tách `plan_session` (thuần, tất định, **test được**) khỏi `run_watch_session` (thực
//! thi: adb + sleep).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::humanize::{self, Rng};
use crate::state::{now_ms, SharedState};

/// Cấu hình phiên xem (FE truyền hoặc dùng mặc định).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchConfig {
    pub min_videos: u32,
    pub max_videos: u32,
    pub min_watch_ms: u64,
    pub max_watch_ms: u64,
    /// Xác suất like mỗi video [0..1].
    pub like_prob: f64,
    /// Xác suất "lướt nhanh" (skip <2s) mỗi video [0..1].
    pub skip_prob: f64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            min_videos: 5,
            max_videos: 12,
            min_watch_ms: 3_000,
            max_watch_ms: 18_000,
            like_prob: 0.12,
            skip_prob: 0.15,
        }
    }
}

/// Hành động cho MỘT video trong phiên.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoAction {
    pub watch_ms: u64,
    pub like: bool,
}

/// Báo cáo phiên (trả về FE qua sự kiện).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReport {
    pub index: u32,
    pub videos: usize,
    pub liked: usize,
    pub duration_ms: u64,
}

/// Lập kế hoạch phiên — **thuần & tất định theo `rng`** (test được, không side-effect).
pub fn plan_session(cfg: &WatchConfig, rng: &mut Rng) -> Vec<VideoAction> {
    let lo = cfg.min_videos.min(cfg.max_videos).max(1);
    let hi = cfg.min_videos.max(cfg.max_videos).max(1);
    let n = rng.irange(lo as i32, hi as i32) as usize;
    let wlo = cfg.min_watch_ms.min(cfg.max_watch_ms).max(1);
    let whi = cfg.min_watch_ms.max(cfg.max_watch_ms).max(1);
    (0..n)
        .map(|_| {
            let skip = rng.f() < cfg.skip_prob;
            let watch_ms = if skip {
                rng.urange(600, 2_000)
            } else {
                rng.urange(wlo, whi)
            };
            // Ít khi like video vừa lướt nhanh (giống người: like cái mình xem kỹ).
            let like = !skip && rng.f() < cfg.like_prob;
            VideoAction { watch_ms, like }
        })
        .collect()
}

/// Chạy phiên xem: mỗi video → xem (ngẫu nhiên) → (đôi khi) like double-tap giữa →
/// cuộn sang video kế (swipe giữa) → nghỉ ngắn. Trả báo cáo.
pub async fn run_watch_session(
    state: &SharedState,
    idx: u32,
    cfg: WatchConfig,
) -> AppResult<SessionReport> {
    // Kích thước màn hình từ fingerprint đã lưu (khớp độ phân giải đã inject); fallback 1080x1920.
    let (w, h) = state
        .hardware_of(idx)
        .await
        .map(|hw| (hw.res_width as i32, hw.res_height as i32))
        .unwrap_or((1080, 1920));
    let (cx, cy) = (w / 2, h / 2);

    let mut rng = Rng::from_entropy();
    let plan = plan_session(&cfg, &mut rng);
    let start = now_ms();
    let mut liked = 0usize;

    for action in &plan {
        // Xem video.
        tokio::time::sleep(Duration::from_millis(action.watch_ms)).await;
        if action.like {
            // Double-tap GIỮA màn hình = like (không cần tọa độ nút like).
            state.adb.human_tap(idx, cx, cy).await?;
            tokio::time::sleep(Duration::from_millis(rng.urange(60, 120))).await;
            state.adb.human_tap(idx, cx, cy).await?;
            liked += 1;
        }
        // Cuộn sang video kế: swipe từ 72% → 28% chiều cao (giữa theo chiều ngang).
        state
            .adb
            .human_swipe(idx, cx, h * 72 / 100, cx, h * 28 / 100)
            .await?;
        // Nghỉ ngắn kiểu người trước video kế.
        tokio::time::sleep(Duration::from_millis(humanize::human_delay_ms(
            700, &mut rng,
        )))
        .await;
    }

    Ok(SessionReport {
        index: idx,
        videos: plan.len(),
        liked,
        duration_ms: (now_ms() - start).max(0) as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_tat_dinh_theo_seed() {
        let cfg = WatchConfig::default();
        let a = plan_session(&cfg, &mut Rng::new(1));
        let b = plan_session(&cfg, &mut Rng::new(1));
        assert_eq!(a, b, "cùng seed → cùng kế hoạch");
    }

    #[test]
    fn so_video_va_thoi_luong_trong_bien() {
        let cfg = WatchConfig::default();
        let mut rng = Rng::new(42);
        for _ in 0..50 {
            let plan = plan_session(&cfg, &mut rng);
            assert!(
                (5..=12).contains(&plan.len()),
                "số video 5-12: {}",
                plan.len()
            );
            for a in &plan {
                // Skip <2s hoặc xem 3-18s.
                assert!(
                    (600..=2_000).contains(&a.watch_ms) || (3_000..=18_000).contains(&a.watch_ms),
                    "watch_ms hợp lệ: {}",
                    a.watch_ms
                );
            }
        }
    }

    #[test]
    fn like_prob_0_khong_like_prob_1_like_video_xem_ky() {
        let mut rng = Rng::new(7);
        let none = plan_session(
            &WatchConfig {
                like_prob: 0.0,
                skip_prob: 0.0,
                ..Default::default()
            },
            &mut rng,
        );
        assert!(none.iter().all(|a| !a.like), "like_prob=0 → không like");

        let all = plan_session(
            &WatchConfig {
                like_prob: 1.0,
                skip_prob: 0.0, // không skip → mọi video đều xem kỹ
                ..Default::default()
            },
            &mut rng,
        );
        assert!(
            all.iter().all(|a| a.like),
            "like_prob=1 + không skip → like tất cả"
        );
    }

    #[test]
    fn video_skip_khong_bao_gio_like() {
        // skip_prob=1 → mọi video lướt nhanh → không like dù like_prob=1.
        let all_skip = plan_session(
            &WatchConfig {
                like_prob: 1.0,
                skip_prob: 1.0,
                ..Default::default()
            },
            &mut Rng::new(9),
        );
        assert!(
            all_skip.iter().all(|a| !a.like && a.watch_ms <= 2_000),
            "video lướt nhanh không like"
        );
    }
}
