//! Polling Service (§7.2, §8.3 SRS). Định kỳ gọi `list_instances` — nguồn sự thật
//! của trạng thái — rồi cập nhật registry và đẩy sự kiện `instances:update` lên FE.
//! Chạy trên interval riêng, độc lập với hàng đợi lệnh.

use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::time::interval;

use crate::model::InstancesUpdateEvent;
use crate::state::SharedState;

pub const INSTANCES_UPDATE_EVENT: &str = "instances:update";

/// Khởi động vòng lặp polling nền. Không bao giờ trả về (chạy suốt đời app).
pub async fn run(app: AppHandle, state: SharedState) {
    // Chặn 0 (interval(0) panic) và giá trị quá nhỏ gây quá tải.
    let read_period = |ms: u32| Duration::from_millis(ms.max(250) as u64);
    let mut current = { state.settings.lock().await.poll_interval_ms };
    let mut ticker = interval(read_period(current));

    loop {
        ticker.tick().await;

        // Áp NGAY khi người dùng đổi chu kỳ trong Settings (không cần restart app).
        let latest = { state.settings.lock().await.poll_interval_ms };
        if latest != current {
            current = latest;
            ticker = interval(read_period(current));
            ticker.tick().await; // bỏ tick tức thì đầu tiên của ticker mới
        }
        match state.memuc.list_instances().await {
            Ok(list) => {
                let mut instances = state.merge_metadata(list).await;

                // Tự nhận quốc gia theo IP thực cho VM đang chạy chưa có country,
                // rồi lưu vào CSDL (chỉ tra 1 lần/VM).
                for inst in &mut instances {
                    if inst.country.is_none() {
                        if let Some(ip) = inst.ip.clone() {
                            if let Some(cc) = state.geo.country(&ip).await {
                                if state.set_country_if_empty(inst.index, cc.clone()).await {
                                    inst.country = Some(cc);
                                }
                            }
                        }
                    }
                }

                {
                    let mut reg = state.registry.lock().await;
                    *reg = instances.clone();
                }
                if let Err(e) = app.emit(INSTANCES_UPDATE_EVENT, InstancesUpdateEvent { instances })
                {
                    tracing::warn!(error = %e, "Không emit được instances:update");
                }
            }
            Err(e) => {
                // Lỗi polling không làm sập app (NFR-R1) — chỉ log và thử lại lần sau.
                tracing::warn!(error = %e, "Polling listvms thất bại");
            }
        }
    }
}
