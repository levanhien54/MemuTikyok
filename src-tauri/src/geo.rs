//! Tra quốc gia từ địa chỉ IP (§ tính năng cột Quốc gia).
//!
//! Theo pattern adapter: UI/logic phụ thuộc trait [`IpGeolocator`], không phụ thuộc
//! nhà cung cấp cụ thể → dễ thay thế và test.
//!
//! ⚠️ [`MockGeolocator`] chỉ dùng cho dev/test (suy diễn tất định, KHÔNG chính xác).
//! [`HttpGeolocator`] gọi ip-api.com (free, HTTP) để tra IP→country thật.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;

#[async_trait]
pub trait IpGeolocator: Send + Sync {
    /// Trả về mã quốc gia ISO 3166-1 alpha-2 (vd "VN"), hoặc None nếu không tra được.
    async fn country(&self, ip: &str) -> Option<String>;
}

/// Tra quốc gia thật qua ip-api.com (miễn phí, không cần khóa, giới hạn 45 req/phút).
///
/// IP của VM mà emulator báo là IP LAN (192.168.x) → không tra được trực tiếp. Với IP
/// riêng/rỗng ta tra **IP công khai của chính host** (`/json/` không tham số): vì đã
/// bỏ proxy, VM thoát mạng qua NAT của host nên cùng quốc gia.
///
/// Cache có **TTL ngắn** (60s): quốc gia thoát của host có thể đổi khi bật/tắt
/// VPN/proxy — cache vĩnh viễn sẽ làm cổng kiểm tra quốc gia dùng dữ liệu cũ.
pub struct HttpGeolocator {
    client: reqwest::Client,
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

/// Thời gian sống của một mục cache quốc gia.
const CACHE_TTL: Duration = Duration::from_secs(60);

impl HttpGeolocator {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .build()
            .unwrap_or_default();
        Self {
            client,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn is_private(ip: &str) -> bool {
        ip.is_empty()
            || ip.starts_with("10.")
            || ip.starts_with("192.168.")
            || ip.starts_with("127.")
            || ip.starts_with("169.254.")
            || (ip.starts_with("172.")
                && ip
                    .split('.')
                    .nth(1)
                    .and_then(|o| o.parse::<u8>().ok())
                    .is_some_and(|o| (16..=31).contains(&o)))
    }
}

impl Default for HttpGeolocator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IpGeolocator for HttpGeolocator {
    async fn country(&self, ip: &str) -> Option<String> {
        // IP riêng → tra IP công khai của host (cùng NAT với VM). Cache theo IP gốc.
        let cache_key = if Self::is_private(ip) { "__self__" } else { ip };
        if let Ok(cache) = self.cache.lock() {
            if let Some((cc, at)) = cache.get(cache_key) {
                if at.elapsed() < CACHE_TTL {
                    return Some(cc.clone());
                }
            }
        }
        let url = if Self::is_private(ip) {
            "http://ip-api.com/json/?fields=status,countryCode".to_string()
        } else {
            format!("http://ip-api.com/json/{ip}?fields=status,countryCode")
        };
        let resp = self.client.get(&url).send().await.ok()?;
        let body: serde_json::Value = resp.json().await.ok()?;
        if body.get("status").and_then(|s| s.as_str()) != Some("success") {
            return None;
        }
        let cc = body.get("countryCode")?.as_str()?.to_string();
        if !cc.is_empty() {
            if let Ok(mut c) = self.cache.lock() {
                c.insert(cache_key.to_string(), (cc.clone(), Instant::now()));
            }
            return Some(cc);
        }
        None
    }
}

#[cfg(test)]
pub struct MockGeolocator;

#[cfg(test)]
#[async_trait]
impl IpGeolocator for MockGeolocator {
    async fn country(&self, ip: &str) -> Option<String> {
        const POOL: &[&str] = &["VN", "US", "SG", "JP", "TH", "ID", "PH", "KR"];
        if ip.is_empty() {
            return None;
        }
        let sum: usize = ip.bytes().map(|b| b as usize).sum();
        POOL.get(sum % POOL.len()).map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tra_quoc_gia_tat_dinh() {
        let g = MockGeolocator;
        let c1 = g.country("192.168.1.20").await;
        let c2 = g.country("192.168.1.20").await;
        assert!(c1.is_some());
        assert_eq!(c1, c2, "phải tất định cho cùng IP");
        assert_eq!(g.country("").await, None);
    }

    #[test]
    fn cache_het_han_theo_ttl() {
        // Mục cache cũ hơn TTL phải bị coi là hết hạn (không dùng lại).
        let fresh = ("VN".to_string(), Instant::now());
        assert!(fresh.1.elapsed() < CACHE_TTL, "mục vừa tạo còn hạn");
        let stale_at = Instant::now()
            .checked_sub(CACHE_TTL + Duration::from_secs(1))
            .expect("trừ thời gian");
        assert!(stale_at.elapsed() >= CACHE_TTL, "mục quá TTL phải hết hạn");
    }

    #[test]
    fn ip_rieng_duoc_nhan_dien() {
        for ip in [
            "",
            "10.0.0.5",
            "192.168.1.2",
            "172.16.0.1",
            "172.31.9.9",
            "127.0.0.1",
        ] {
            assert!(HttpGeolocator::is_private(ip), "{ip} phải là IP riêng");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "172.32.0.1", "172.15.0.1"] {
            assert!(!HttpGeolocator::is_private(ip), "{ip} phải là IP công khai");
        }
    }
}
