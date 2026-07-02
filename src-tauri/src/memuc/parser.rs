//! Parser output `memuc listvms` (§7.3 SRS).
//!
//! Định dạng mỗi dòng (CSV): `index,title,top-handle,status,pid,disk-usage`
//! Ví dụ: `0,MEmu,197666,1,10508,4096`
//!
//! Nguyên tắc: **pure function**, **fault-tolerant** — dòng hỏng bị bỏ qua
//! (kèm cảnh báo) thay vì làm sập toàn bộ. Đây là ranh giới cô lập giúp thích ứng
//! khi MEmu đổi format (R-07).

use crate::model::{Instance, InstanceStatus};

/// Chuyển trường "status" của memuc sang enum. Quy ước: 1 = đang chạy.
fn parse_status(raw: &str) -> InstanceStatus {
    match raw.trim() {
        "1" => InstanceStatus::Running,
        _ => InstanceStatus::Stopped,
    }
}

/// `0` hoặc rỗng được coi là "không có" (None).
fn parse_optional_u32(raw: &str) -> Option<u32> {
    match raw.trim().parse::<u32>() {
        Ok(0) | Err(_) => None,
        Ok(v) => Some(v),
    }
}

fn parse_optional_i64(raw: &str) -> Option<i64> {
    match raw.trim().parse::<i64>() {
        Ok(0) | Err(_) => None,
        Ok(v) => Some(v),
    }
}

fn parse_optional_u64(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok()
}

/// Parse một dòng. Trả về `None` nếu dòng không hợp lệ (thiếu trường tối thiểu
/// hoặc index không phải số).
fn parse_line(line: &str) -> Option<Instance> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let fields: Vec<&str> = line.split(',').collect();
    // Tối thiểu cần: index, title, handle, status
    if fields.len() < 4 {
        tracing::warn!(line, "Bỏ qua dòng listvms thiếu trường");
        return None;
    }

    let index = match fields[0].trim().parse::<u32>() {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(line, "Bỏ qua dòng listvms có index không hợp lệ");
            return None;
        }
    };

    Some(Instance {
        index,
        title: fields[1].trim().to_string(),
        status: parse_status(fields[3]),
        window_handle: parse_optional_i64(fields[2]),
        pid: fields.get(4).and_then(|s| parse_optional_u32(s)),
        disk_usage_bytes: fields.get(5).and_then(|s| parse_optional_u64(s)),
        ip: None, // IP lấy riêng qua adb khi cần (FR-A-3)
        // Các trường sau do metadata store cung cấp (merge sau khi list).
        last_launched_at: None,
        country: None,
        note: String::new(),
        account: None,
    })
}

/// Parse toàn bộ stdout của `memuc listvms` thành danh sách instance.
pub fn parse_listvms(stdout: &str) -> Vec<Instance> {
    stdout.lines().filter_map(parse_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dong_co_ban() {
        let out = "0,MEmu,197666,1,10508,4096";
        let vms = parse_listvms(out);
        assert_eq!(vms.len(), 1);
        let vm = &vms[0];
        assert_eq!(vm.index, 0);
        assert_eq!(vm.title, "MEmu");
        assert_eq!(vm.status, InstanceStatus::Running);
        assert_eq!(vm.window_handle, Some(197666));
        assert_eq!(vm.pid, Some(10508));
        assert_eq!(vm.disk_usage_bytes, Some(4096));
    }

    #[test]
    fn vm_dung_khong_co_pid_handle() {
        let out = "1,MEmu_1,0,0,0,2048";
        let vms = parse_listvms(out);
        assert_eq!(vms.len(), 1);
        let vm = &vms[0];
        assert_eq!(vm.status, InstanceStatus::Stopped);
        assert_eq!(vm.window_handle, None);
        assert_eq!(vm.pid, None);
    }

    #[test]
    fn nhieu_dong() {
        let out = "0,MEmu,197666,1,10508,4096\n1,MEmu_1,0,0,0,2048\n2,Tester,0,0,0,0";
        let vms = parse_listvms(out);
        assert_eq!(vms.len(), 3);
        assert_eq!(vms[2].title, "Tester");
        assert_eq!(vms[2].disk_usage_bytes, Some(0));
    }

    #[test]
    fn output_rong_tra_ve_danh_sach_rong() {
        assert!(parse_listvms("").is_empty());
        assert!(parse_listvms("\n\n  \n").is_empty());
    }

    #[test]
    fn dong_hong_bi_bo_qua_khong_lam_sap() {
        // Dòng 2 hỏng (index không phải số), dòng 3 thiếu trường.
        let out = "0,MEmu,197666,1,10508,4096\nrác,dữ,liệu\n3,OnlyThree";
        let vms = parse_listvms(out);
        // Chỉ dòng đầu hợp lệ; dòng cuối thiếu trường (chỉ 2) bị loại.
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].index, 0);
    }

    #[test]
    fn thieu_truong_tuy_chon_van_parse_duoc() {
        // Chỉ có 4 trường tối thiểu.
        let out = "5,Sandbox,0,1";
        let vms = parse_listvms(out);
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].status, InstanceStatus::Running);
        assert_eq!(vms[0].pid, None);
        assert_eq!(vms[0].disk_usage_bytes, None);
    }

    #[test]
    fn title_co_khoang_trang_duoc_giu() {
        let out = "0, My VM ,100,0";
        let vms = parse_listvms(out);
        assert_eq!(vms[0].title, "My VM");
    }
}
