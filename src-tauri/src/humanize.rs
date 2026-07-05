//! Sinh chuỗi input **GIẢ NGƯỜI** để chống phát hiện tự động hóa.
//!
//! TikTok/ByteDance chấm điểm bot qua **touch micro-jitter** (vận tốc/áp lực), tọa độ
//! chính xác tuyệt đối và timing đều tăm tắp của `input tap`/`swipe` thô. Module này
//! sinh: (a) tap rung tọa độ + thời gian giữ ngẫu nhiên, (b) swipe theo **đường Bézier
//! cong** với vận tốc ease-in-out + nhiễu (không phải đường thẳng vận tốc đều), (c) nhịp
//! chờ giữa hành động kiểu người.
//!
//! Phần SINH ở đây là **thuần & tất định (seed được)** → test được. Phần thực thi (adb
//! `input`) nằm ở `adb.rs`.

/// PRNG xorshift* nhẹ, **seed được** (test tất định). KHÔNG dùng cho mật mã.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// Seed từ entropy hệ thống (dùng lúc chạy thật).
    pub fn from_entropy() -> Self {
        let mut b = [0u8; 8];
        let _ = getrandom::getrandom(&mut b);
        Self::new(u64::from_le_bytes(b))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// f64 trong [0, 1).
    pub fn f(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// f64 trong [a, b).
    fn frange(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f()
    }

    /// i32 trong [a, b] (bao gồm hai đầu).
    pub fn irange(&mut self, a: i32, b: i32) -> i32 {
        debug_assert!(b >= a);
        a + (self.next_u64() % ((b - a + 1) as u64)) as i32
    }

    /// u64 trong [a, b] (bao gồm hai đầu).
    pub fn urange(&mut self, a: u64, b: u64) -> u64 {
        debug_assert!(b >= a);
        a + self.next_u64() % (b - a + 1)
    }
}

/// Một bước chạm: tọa độ + số ms chờ TRƯỚC khi di tới điểm này (điều khiển vận tốc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchStep {
    pub x: i32,
    pub y: i32,
    pub delay_ms: u64,
}

/// Tap giả người: rung tọa độ ±3px + thời gian giữ ngón (press) ngẫu nhiên 60–140ms.
pub fn human_tap(x: i32, y: i32, rng: &mut Rng) -> (i32, i32, u64) {
    let jx = rng.irange(-3, 3);
    let jy = rng.irange(-3, 3);
    let hold = rng.irange(60, 140) as u64;
    (x + jx, y + jy, hold)
}

/// Swipe giả người: đường **Bézier bậc 2** với điểm control lệch vuông góc ngẫu nhiên
/// (đường cong), lấy mẫu 16–26 điểm theo **smoothstep** (chậm hai đầu, nhanh giữa =
/// vận tốc ease-in-out), rung nhẹ mỗi điểm, tổng thời gian 180–420ms.
pub fn human_swipe(from: (i32, i32), to: (i32, i32), rng: &mut Rng) -> Vec<TouchStep> {
    let steps = rng.irange(16, 26) as usize;
    let (x0, y0) = (from.0 as f64, from.1 as f64);
    let (x1, y1) = (to.0 as f64, to.1 as f64);

    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    // Vector pháp tuyến (vuông góc) đã chuẩn hóa → lệch điểm control tạo độ cong.
    let (px, py) = (-dy / len, dx / len);
    let sign = if rng.f() < 0.5 { -1.0 } else { 1.0 };
    let bow = rng.frange(0.06, 0.18) * len * sign;
    let cx = (x0 + x1) / 2.0 + px * bow;
    let cy = (y0 + y1) / 2.0 + py * bow;

    let total_ms = rng.irange(180, 420) as u64;
    let per = (total_ms / steps as u64).max(4);

    let mut out = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        // smoothstep → vận tốc ease-in-out; nhiễu nhỏ để không hoàn hảo. Ghim ĐÚNG hai
        // đầu (te=0/1) để swipe bắt đầu/kết thúc đúng điểm; chỉ nhiễu ở giữa.
        let te = if i == 0 {
            0.0
        } else if i == steps {
            1.0
        } else {
            (t * t * (3.0 - 2.0 * t) + rng.frange(-0.015, 0.015)).clamp(0.0, 1.0)
        };
        let omt = 1.0 - te;
        let bx = omt * omt * x0 + 2.0 * omt * te * cx + te * te * x1;
        let by = omt * omt * y0 + 2.0 * omt * te * cy + te * te * y1;
        let jx = rng.frange(-1.5, 1.5);
        let jy = rng.frange(-1.5, 1.5);
        let delay = if i == 0 {
            0
        } else {
            (per as i64 + rng.irange(-3, 3) as i64).max(3) as u64
        };
        out.push(TouchStep {
            x: (bx + jx).round() as i32,
            y: (by + jy).round() as i32,
            delay_ms: delay,
        });
    }
    out
}

/// Nhịp chờ giữa các hành động (xem/đọc) kiểu người: quanh `base` ms, biến thiên ±40%.
pub fn human_delay_ms(base: u64, rng: &mut Rng) -> u64 {
    let lo = (base as f64 * 0.6) as i32;
    let hi = (base as f64 * 1.4) as i32;
    rng.irange(lo.max(1), hi.max(2)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_tat_dinh_theo_seed() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        assert_eq!(a.next_u64(), b.next_u64(), "cùng seed → cùng chuỗi");
        let mut c = Rng::new(43);
        assert_ne!(a.next_u64(), c.next_u64(), "seed khác → chuỗi khác");
    }

    #[test]
    fn tap_rung_trong_bien_va_giu_hop_ly() {
        let mut rng = Rng::new(7);
        for _ in 0..100 {
            let (x, y, hold) = human_tap(500, 900, &mut rng);
            assert!((497..=503).contains(&x), "x rung ±3: {x}");
            assert!((897..=903).contains(&y), "y rung ±3: {y}");
            assert!((60..=140).contains(&hold), "giữ 60-140ms: {hold}");
        }
    }

    #[test]
    fn swipe_dung_diem_dau_cuoi_va_cong() {
        let mut rng = Rng::new(123);
        let from = (540, 1600);
        let to = (540, 400);
        let path = human_swipe(from, to, &mut rng);
        assert!(path.len() >= 17, "≥17 điểm");
        // Điểm đầu/cuối gần from/to (trong biên rung ~2px).
        assert!((path[0].x - from.0).abs() <= 2 && (path[0].y - from.1).abs() <= 2);
        let last = path.last().unwrap();
        assert!((last.x - to.0).abs() <= 2 && (last.y - to.1).abs() <= 2);
        // step 0 không chờ; các step sau đều có delay > 0.
        assert_eq!(path[0].delay_ms, 0);
        assert!(path[1..].iter().all(|s| s.delay_ms >= 3));
        // Đường CONG: có điểm lệch khỏi đường thẳng from→to (x không cố định 540).
        let curved = path.iter().any(|s| (s.x - 540).abs() > 3);
        assert!(curved, "swipe phải cong (không phải đường thẳng)");
        // Tổng thời gian trong khoảng hợp lý.
        let total: u64 = path.iter().map(|s| s.delay_ms).sum();
        assert!(
            (120..=650).contains(&total),
            "tổng thời gian swipe hợp lý: {total}"
        );
    }

    #[test]
    fn delay_bien_thien_quanh_base() {
        let mut rng = Rng::new(9);
        let mut seen_lo = false;
        let mut seen_hi = false;
        for _ in 0..200 {
            let d = human_delay_ms(1000, &mut rng);
            assert!((600..=1400).contains(&d), "trong ±40%: {d}");
            if d < 900 {
                seen_lo = true;
            }
            if d > 1100 {
                seen_hi = true;
            }
        }
        assert!(seen_lo && seen_hi, "phải biến thiên hai phía");
    }
}
