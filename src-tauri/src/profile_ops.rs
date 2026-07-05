//! Lõi nghiệp vụ vòng đời PROFILE (disposable: profile = dữ liệu bền, VM = pool tạm).
//!
//! Tách khỏi lớp `#[tauri::command]` (commands.rs) để test được TRỰC TIẾP — kể cả
//! E2E trên MEmu THẬT — mà không phải dựng `tauri::State`. Các lệnh Tauri chỉ là
//! adapter mỏng gọi vào đây, nên test ở đây kiểm đúng code chạy production.

use crate::error::{AppError, AppResult};
use crate::model::{AccountProfile, Profile, ProfileView, SnapshotRecord, DEFAULT_TIKTOK_APK, TIKTOK_PKG};
use crate::orchestrator;
use crate::state::{now_ms, RunSlot, SharedState};

/// Số VM chạy đồng thời tối đa (kế hoạch §18: tối đa ~5 VM).
pub const MAX_RUNNING_VMS: usize = 5;

/// Kiểm tra tên profile — dùng ĐỒNG THỜI làm PRIMARY KEY (SQLite), KHÓA SNAPSHOT
/// (đường dẫn hệ tệp) và tham số shell. Whitelist NGHIÊM: `[A-Za-z0-9._-]`, ≤64 ký tự,
/// cấm `..`. Chống path-traversal (username="../.." thoát thư mục snapshot), injection,
/// và nhầm-vai-trò. Trả tên đã trim khi hợp lệ.
fn validate_username(name: &str) -> AppResult<String> {
    let u = name.trim();
    if u.is_empty() {
        return Err(AppError::InvalidInput(
            "Tên tài khoản không được rỗng".into(),
        ));
    }
    if u.chars().count() > 64 {
        return Err(AppError::InvalidInput(
            "Tên tài khoản quá dài (tối đa 64 ký tự)".into(),
        ));
    }
    if u == "." || u.contains("..") {
        return Err(AppError::InvalidInput("Tên tài khoản không hợp lệ".into()));
    }
    if !u
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(AppError::InvalidInput(
            "Tên tài khoản chỉ gồm chữ/số và . _ - (không dấu cách hay ký tự đặc biệt)".into(),
        ));
    }
    Ok(u.to_string())
}

/// Đường dẫn APK TikTok từ settings (fallback mặc định) — dùng khi provision cài app.
async fn apk_path(state: &SharedState) -> String {
    state
        .settings
        .lock()
        .await
        .tiktok_apk_path
        .clone()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TIKTOK_APK.to_string())
}

/// Tạo PROFILE mới — CHỈ ghi dữ liệu (account + fingerprint), KHÔNG tạo VM.
pub async fn create(
    state: &SharedState,
    account: AccountProfile,
    note: Option<String>,
    country: Option<String>,
) -> AppResult<String> {
    let username = validate_username(&account.tiktok_username)?;
    if state.get_profile(&username).await.is_some() {
        return Err(AppError::InvalidInput("Profile tên này đã tồn tại".into()));
    }
    let hardware = crate::fingerprint::generate()?;
    let profile = Profile {
        username: username.clone(),
        account: AccountProfile {
            tiktok_username: username.clone(),
            ..account
        },
        hardware,
        country: country
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty()),
        note: note.unwrap_or_default().trim().to_string(),
        created_at: now_ms(),
        last_run_at: None,
    };
    state.upsert_profile(profile).await;
    Ok(username)
}

/// Danh sách profile + trạng thái runtime (đang chạy trên VM nào).
pub async fn list(state: &SharedState) -> Vec<ProfileView> {
    let mut views = Vec::new();
    for p in state.list_profiles().await {
        let running_vm = state.running_vm_of(&p.username).await;
        views.push(ProfileView {
            profile: p,
            running_vm,
        });
    }
    views
}

/// Cập nhật account/ghi chú/quốc gia của profile (giữ nguyên username-key).
pub async fn update(
    state: &SharedState,
    username: &str,
    account: AccountProfile,
    note: String,
    country: Option<String>,
) -> AppResult<()> {
    let mut p = state
        .get_profile(username)
        .await
        .ok_or_else(|| AppError::InvalidInput("Không tìm thấy profile".into()))?;
    p.account = AccountProfile {
        tiktok_username: username.to_string(),
        ..account
    };
    p.note = note.trim().to_string();
    p.country = country
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty());
    state.upsert_profile(p).await;
    Ok(())
}

/// CHẠY profile: cổng quốc gia → ĐẶT CHỖ nguyên tử (idempotency + ≤ MAX) → provision
/// VM sạch (áp fingerprint + cài TikTok + restore session theo username) → mở TikTok →
/// ghi running map + cập nhật last_run_at. Idempotent: đang chạy → trả VM hiện tại.
pub async fn run(state: &SharedState, username: &str) -> AppResult<u32> {
    let profile = state
        .get_profile(username)
        .await
        .ok_or_else(|| AppError::InvalidInput("Không tìm thấy profile".into()))?;
    // Đã chạy rồi → trả VM hiện tại ngay (không tra mạng, không cấp thêm).
    if let Some(vm) = state.running_vm_of(username).await {
        return Ok(vm);
    }
    // Cổng quốc gia (validation TRƯỚC khi chiếm slot): IP thoát thực tế phải khớp.
    // ⚠️ Đây là kiểm IP thoát của HOST (mọi VM chung NAT host) — phép kiểm VPN mức-host,
    // KHÔNG phải cách ly geo per-account. Nhiều account cùng 1 IP vẫn liên-kết được.
    if let Some(expected) = profile.country.as_deref().filter(|c| !c.is_empty()) {
        match state.geo.country("").await {
            Some(actual) if actual.eq_ignore_ascii_case(expected) => {}
            Some(actual) => {
                return Err(AppError::CountryMismatch {
                    actual: actual.to_uppercase(),
                    expected: expected.to_string(),
                })
            }
            None => return Err(AppError::CountryUnverified(expected.to_string())),
        }
    }
    // NGUYÊN TỬ: idempotency + cổng tối đa + ĐẶT CHỖ slot dưới MỘT khóa (chống đua:
    // nhiều run song song không cùng vượt cổng; cùng username không provision đôi).
    match state.reserve_run_slot(username, MAX_RUNNING_VMS).await {
        RunSlot::AlreadyRunning(vm) => return Ok(vm),
        RunSlot::Pending => {
            return Err(AppError::InvalidInput(
                "Profile đang được khởi chạy — vui lòng đợi".into(),
            ))
        }
        RunSlot::AtCapacity => {
            return Err(AppError::InvalidInput(format!(
                "Đã đạt tối đa {MAX_RUNNING_VMS} VM chạy đồng thời — dừng bớt profile khác trước"
            )))
        }
        RunSlot::Reserved => {}
    }
    // Từ đây đã giữ chỗ → mọi nhánh thoát PHẢI nhả chỗ (clear_running_profile).
    // APK fail-fast: kiểm tra tồn tại TRƯỚC khi tốn công tạo/boot VM.
    let apk = apk_path(state).await;
    if !std::path::Path::new(&apk).exists() {
        state.clear_running_profile(username).await;
        return Err(AppError::InvalidInput(format!(
            "Không tìm thấy APK TikTok tại: {apk} — vào Cài đặt đặt 'Đường dẫn APK TikTok'"
        )));
    }
    // Provision (ngoài khóa — thao tác dài). Lỗi → NHẢ chỗ đã đặt.
    let idx = match orchestrator::provision(state, username, &profile.hardware, Some(&apk)).await {
        Ok(i) => i,
        Err(e) => {
            state.clear_running_profile(username).await;
            return Err(e);
        }
    };
    let _ = state.adb.start_app(idx, TIKTOK_PKG).await;
    state.set_running_profile(username, idx).await; // ghi đè RESERVED bằng idx thật
    let mut p = profile;
    p.last_run_at = Some(now_ms());
    state.upsert_profile(p).await;
    Ok(idx)
}

/// DỪNG profile: backup session → HỦY VM (disposable) → nhả running map. Trả
/// snapshot record nếu đang chạy; `None` nếu profile không chạy (idempotent).
pub async fn stop(state: &SharedState, username: &str) -> AppResult<Option<SnapshotRecord>> {
    // Lấy-và-xóa NGUYÊN TỬ (chỉ một caller thắng entry → chống teardown đôi).
    let Some(idx) = state.take_running_vm(username).await else {
        return Ok(None);
    };
    match orchestrator::teardown(state, idx, username).await {
        Ok(rec) => Ok(Some(rec)),
        // Teardown lỗi (vd backup fail) → TÁI theo dõi để retry, KHÔNG bỏ VM khỏi map
        // (nếu bỏ, VM còn chạy nhưng vô chủ, không tính vào cổng, không ai dọn).
        Err(e) => {
            state.set_running_profile(username, idx).await;
            Err(e)
        }
    }
}

/// XÓA profile: nếu đang chạy thì teardown trước (backup + hủy VM), rồi xóa bản ghi.
pub async fn delete(state: &SharedState, username: &str) -> AppResult<()> {
    if let Some(idx) = state.take_running_vm(username).await {
        if let Err(e) = orchestrator::teardown(state, idx, username).await {
            // Teardown lỗi → giữ tracking, KHÔNG xóa profile khi VM còn sống.
            state.set_running_profile(username, idx).await;
            return Err(e);
        }
    }
    state.delete_profile(username).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::MockAdbWorker;
    use crate::db::Db;
    use crate::geo::{IpGeolocator, MockGeolocator};
    use crate::memuc::MockMemuc;
    use crate::model::AppSettings;
    use crate::snapshot::LocalSnapshotStore;
    use crate::state::AppState;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Geo trả về quốc gia cố định (hoặc None) — test cổng quốc gia của run().
    struct FixedGeo(Option<&'static str>);
    #[async_trait::async_trait]
    impl IpGeolocator for FixedGeo {
        async fn country(&self, _ip: &str) -> Option<String> {
            self.0.map(|s| s.to_string())
        }
    }

    fn acc(username: &str) -> AccountProfile {
        AccountProfile {
            tiktok_username: username.into(),
            tiktok_password: String::new(),
            two_fa: String::new(),
            tiktok_passkey: String::new(),
            email: String::new(),
            email_password: String::new(),
        }
    }

    fn make_state(tag: &str, geo: Arc<dyn IpGeolocator>) -> SharedState {
        let memuc = Arc::new(MockMemuc::new());
        let adb = Arc::new(MockAdbWorker::new());
        let dir = std::env::temp_dir().join(format!("mpm_pops_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Arc::new(LocalSnapshotStore::new(dir.join("snap"), Some([5u8; 32])).unwrap());
        let db = Db::open_with_key(&dir.join("mpm.db"), None).unwrap();
        Arc::new(AppState::new(
            memuc,
            geo,
            adb,
            store,
            AppSettings::default(),
            Some(db),
            HashMap::new(),
        ))
    }

    #[tokio::test]
    async fn create_tu_choi_username_rong() {
        let state = make_state("empty", Arc::new(MockGeolocator));
        let r = create(&state, acc("   "), None, None).await;
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "username rỗng phải lỗi");
    }

    #[tokio::test]
    async fn create_tu_choi_trung_ten() {
        let state = make_state("dup", Arc::new(MockGeolocator));
        create(&state, acc("acc_a"), None, None).await.unwrap();
        let r = create(&state, acc("acc_a"), None, None).await;
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "trùng tên phải lỗi");
    }

    #[tokio::test]
    async fn run_idempotent_khi_dang_chay() {
        // Đang chạy trên VM 42 → run trả đúng 42, KHÔNG provision.
        let state = make_state("idem", Arc::new(MockGeolocator));
        create(&state, acc("acc_r"), None, None).await.unwrap();
        state.set_running_profile("acc_r", 42).await;
        assert_eq!(run(&state, "acc_r").await.unwrap(), 42);
    }

    #[tokio::test]
    async fn run_chan_khi_du_max_vm() {
        // Nạp sẵn MAX running (không boot) → run profile kế → chặn TRƯỚC provision.
        let state = make_state("cap", Arc::new(MockGeolocator));
        create(&state, acc("acc_cap"), None, None).await.unwrap();
        for i in 0..MAX_RUNNING_VMS as u32 {
            state.set_running_profile(&format!("busy_{i}"), 900 + i).await;
        }
        match run(&state, "acc_cap").await {
            Err(AppError::InvalidInput(msg)) => assert!(msg.contains("tối đa"), "msg cap: {msg}"),
            other => panic!("phải chặn vì đủ MAX, nhận: {other:?}"),
        }
        assert!(
            state.running_vm_of("acc_cap").await.is_none(),
            "profile bị cap KHÔNG được đăng ký running"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn run_cap_nguyen_tu_duoi_dua_tranh() {
        // Đua tranh: 8 profile chạy SONG SONG → cổng tối đa 5 phải giữ nguyên tử
        // (không TOCTOU cho 6+ VM qua). Đúng 5 thành công, 3 bị chặn.
        let state = make_state("race", Arc::new(MockGeolocator));
        // APK giả (tồn tại) để qua fail-fast; provision mock không đọc nội dung.
        let apk = std::env::temp_dir().join(format!("mpm_fake_{}.apk", std::process::id()));
        std::fs::write(&apk, b"x").unwrap();
        state.settings.lock().await.tiktok_apk_path = Some(apk.to_string_lossy().to_string());
        for i in 0..8 {
            create(&state, acc(&format!("p{i}")), None, None).await.unwrap();
        }
        let mut handles = Vec::new();
        for i in 0..8 {
            let s = state.clone();
            let name = format!("p{i}");
            handles.push(tokio::spawn(async move { run(&s, &name).await }));
        }
        let (mut ok, mut capped) = (0, 0);
        for h in handles {
            match h.await.unwrap() {
                Ok(_) => ok += 1,
                Err(AppError::InvalidInput(m)) if m.contains("tối đa") => capped += 1,
                other => panic!("kết quả bất ngờ: {other:?}"),
            }
        }
        assert_eq!(ok, MAX_RUNNING_VMS, "đúng {MAX_RUNNING_VMS} chạy được");
        assert_eq!(capped, 8 - MAX_RUNNING_VMS, "phần còn lại bị cổng chặn");
        assert_eq!(
            state.running_profiles.lock().await.len(),
            MAX_RUNNING_VMS,
            "map running không vượt cổng"
        );
        let _ = std::fs::remove_file(&apk);
    }

    #[tokio::test]
    async fn run_chan_khi_lech_quoc_gia() {
        // profile yêu cầu US, IP thoát VN → CountryMismatch.
        let state = make_state("cc_bad", Arc::new(FixedGeo(Some("VN"))));
        create(&state, acc("acc_cc"), None, Some("US".into())).await.unwrap();
        match run(&state, "acc_cc").await {
            Err(AppError::CountryMismatch { actual, expected }) => {
                assert_eq!(actual, "VN");
                assert_eq!(expected, "US");
            }
            other => panic!("phải CountryMismatch, nhận: {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_chan_khi_khong_xac_thuc_quoc_gia() {
        // profile yêu cầu VN nhưng geo không tra được → CountryUnverified (an toàn).
        let state = make_state("cc_unv", Arc::new(FixedGeo(None)));
        create(&state, acc("acc_cc2"), None, Some("VN".into())).await.unwrap();
        assert!(matches!(
            run(&state, "acc_cc2").await,
            Err(AppError::CountryUnverified(cc)) if cc == "VN"
        ));
    }

    #[tokio::test]
    async fn stop_none_khi_khong_chay() {
        let state = make_state("stop", Arc::new(MockGeolocator));
        create(&state, acc("acc_s"), None, None).await.unwrap();
        assert!(stop(&state, "acc_s").await.unwrap().is_none(), "không chạy → None");
    }
}
