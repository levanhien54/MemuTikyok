//! Lõi nghiệp vụ vòng đời PROFILE (disposable: profile = dữ liệu bền, VM = pool tạm).
//!
//! Tách khỏi lớp `#[tauri::command]` (commands.rs) để test được TRỰC TIẾP — kể cả
//! E2E trên MEmu THẬT — mà không phải dựng `tauri::State`. Các lệnh Tauri chỉ là
//! adapter mỏng gọi vào đây, nên test ở đây kiểm đúng code chạy production.

use crate::error::{AppError, AppResult};
use crate::model::{AccountProfile, Profile, ProfileView, SnapshotRecord, DEFAULT_TIKTOK_APK, TIKTOK_PKG};
use crate::orchestrator;
use crate::state::{now_ms, RunSlot, SharedState, RESERVED_VM};

/// Số VM chạy đồng thời tối đa (kế hoạch §18: tối đa ~5 VM).
pub const MAX_RUNNING_VMS: usize = 5;

/// Guard RAII cho slot đã đặt chỗ (`RESERVED_VM`): nếu KHÔNG `commit()` — do lỗi, `?`
/// thoát sớm, panic, hay task bị hủy giữa provision — thì `Drop` tự NHẢ chỗ. Nhờ
/// `running_profiles` là std Mutex nên khóa được đồng bộ trong Drop. Chỉ xóa khi entry
/// VẪN là RESERVED (không đụng idx thật đã set hay entry của caller khác).
struct RunReservation<'a> {
    state: &'a SharedState,
    username: String,
    committed: bool,
}

impl<'a> RunReservation<'a> {
    fn new(state: &'a SharedState, username: &str) -> Self {
        Self {
            state,
            username: username.to_string(),
            committed: false,
        }
    }
    /// Giữ chỗ (đã set idx thật) — Drop sẽ không nhả.
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for RunReservation<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Ok(mut g) = self.state.running_profiles.lock() {
            if g.get(&self.username).copied() == Some(RESERVED_VM) {
                g.remove(&self.username);
            }
        }
    }
}

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
///
/// RECONCILE với nguồn-sự-thật memuc: VM có thể chết NGOÀI luồng app (đóng MEmu, crash,
/// reboot dịch vụ). Nếu vm_index không còn trong `listvms` → nhả tracking + hiện "nghỉ".
/// Chỉ tra memuc khi CÓ profile đang chạy (khỏi tốn lệnh CLI lúc rảnh) và chỉ dọn khi
/// listvms THÀNH CÔNG (lỗi tạm thời không được xóa nhầm trạng thái).
pub async fn list(state: &SharedState) -> Vec<ProfileView> {
    let mut raw: Vec<(Profile, Option<u32>)> = Vec::new();
    let mut has_running = false;
    for p in state.list_profiles().await {
        let vm = state.running_vm_of(&p.username).await;
        has_running |= vm.is_some();
        raw.push((p, vm));
    }
    let live: Option<std::collections::HashSet<u32>> = if has_running {
        state
            .memuc
            .list_instances()
            .await
            .ok()
            .map(|v| v.into_iter().map(|i| i.index).collect())
    } else {
        None
    };
    let mut views = Vec::with_capacity(raw.len());
    for (p, mut vm) in raw {
        if let (Some(idx), Some(live)) = (vm, &live) {
            if !live.contains(&idx) {
                state.clear_running_profile(&p.username).await;
                vm = None;
            }
        }
        views.push(ProfileView {
            profile: p,
            running_vm: vm,
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
    // Đã giữ chỗ RESERVED. Guard RAII nhả chỗ nếu KHÔNG commit (mọi nhánh thoát bất
    // thường: lỗi, `?`, panic, hủy task) → không rò slot cổng tối đa.
    let reservation = RunReservation::new(state, username);

    // APK fail-fast: kiểm tra tồn tại TRƯỚC khi tốn công tạo/boot VM.
    let apk = apk_path(state).await;
    if !std::path::Path::new(&apk).exists() {
        return Err(AppError::InvalidInput(format!(
            "Không tìm thấy APK TikTok tại: {apk} — vào Cài đặt đặt 'Đường dẫn APK TikTok'"
        ))); // reservation.drop() → nhả chỗ
    }
    // Provision (ngoài khóa — thao tác dài). Lỗi → `?` thoát → reservation.drop() nhả chỗ.
    let idx = orchestrator::provision(state, username, &profile.hardware, Some(&apk)).await?;
    let _ = state.adb.start_app(idx, TIKTOK_PKG).await;
    // Re-fetch: `provision` là thao tác DÀI, KHÔNG giữ khóa profiles → profile có thể đã
    // bị SỬA hoặc XÓA trong lúc đó. Ghi lại bằng snapshot `profile` cũ sẽ clobber bản sửa
    // hoặc hồi sinh bản đã xóa. Nên đọc lại bản HIỆN TẠI trước khi finalize.
    match state.get_profile(username).await {
        Some(mut p) => {
            p.last_run_at = Some(now_ms());
            state.upsert_profile(p).await; // ghi bản hiện tại + last_run_at (không clobber)
            state.set_running_profile(username, idx).await; // RESERVED → idx thật
            reservation.commit(); // giữ entry idx — Drop không nhả
            Ok(idx)
        }
        None => {
            // Profile bị XÓA giữa chừng → HỦY VM vừa cấp (khỏi backup — profile đã mất).
            // reservation.drop() nhả chỗ RESERVED (nếu còn). Không mồ côi VM, không hồi sinh.
            let _ = state.memuc.stop(idx).await;
            let _ = state.memuc.remove(idx).await;
            state.forget(idx).await;
            Err(AppError::InvalidInput(
                "Profile đã bị xóa trong lúc khởi chạy — đã hủy VM vừa cấp".into(),
            ))
        }
    }
}

/// DỪNG profile: backup session → HỦY VM (disposable) → nhả running map. Trả
/// snapshot record nếu đang chạy; `None` nếu profile không chạy (idempotent).
pub async fn stop(state: &SharedState, username: &str) -> AppResult<Option<SnapshotRecord>> {
    // Đang provision (RESERVED) → từ chối: chưa có VM thật để dừng.
    if state.is_reserved(username).await {
        return Err(AppError::InvalidInput(
            "Profile đang khởi chạy — vui lòng đợi rồi thử lại".into(),
        ));
    }
    // Lấy-và-xóa NGUYÊN TỬ (chỉ một caller thắng entry → chống teardown đôi).
    let Some(idx) = state.take_running_vm(username).await else {
        return Ok(None);
    };
    match orchestrator::teardown(state, idx, username).await {
        Ok(rec) => Ok(Some(rec)),
        // Teardown lỗi (vd backup fail) → TÁI theo dõi để retry, KHÔNG bỏ VM khỏi map
        // (nếu bỏ, VM còn chạy nhưng vô chủ). Muốn bỏ hẳn dù backup lỗi → dùng Xóa.
        Err(e) => {
            state.set_running_profile(username, idx).await;
            Err(e)
        }
    }
}

/// XÓA profile: người dùng CHỦ ĐÍCH bỏ → cố backup nhưng KHÔNG chặn xóa nếu backup lỗi
/// (force hủy VM). Đây là lối thoát khi một phiên có dữ liệu không backup được.
pub async fn delete(state: &SharedState, username: &str) -> AppResult<()> {
    // Đang provision (RESERVED) → từ chối: đợi run xong đã (tránh xóa lúc đang cấp VM).
    if state.is_reserved(username).await {
        return Err(AppError::InvalidInput(
            "Profile đang khởi chạy — vui lòng đợi rồi xóa".into(),
        ));
    }
    if let Some(idx) = state.take_running_vm(username).await {
        if let Err(e) = orchestrator::teardown(state, idx, username).await {
            // Backup khi xóa lỗi → VẪN hủy VM + xóa profile (force), không để kẹt.
            tracing::warn!(username, error = %e, "backup khi xóa lỗi — force hủy VM + xóa");
            let _ = state.memuc.stop(idx).await;
            let _ = state.memuc.remove(idx).await;
            state.forget(idx).await;
        }
    }
    state.delete_profile(username).await;
    Ok(())
}

/// RECONCILE lúc KHỞI ĐỘNG: `running_profiles` chỉ ở bộ nhớ nên sau crash/tắt-đột-ngột,
/// VM còn sống trong MEmu nhưng MPM đã quên → **mồ côi** (không hiện ở đâu, chiếm slot,
/// ngốn RAM). Đọc lại bảng `running_vms` đã persist; VM nào CÒN trong `listvms` = mồ côi
/// phiên trước → HỦY (session chưa kịp backup coi như bỏ — backup CŨ vẫn còn; đúng tinh
/// thần disposable). Xong xóa sạch bảng. Trả về số VM đã dọn.
///
/// An toàn: chỉ đụng VM có trong bảng persist của MPM (không bao giờ chạm VM người dùng);
/// nếu `listvms` lỗi thì BỎ QUA lần này (không xóa bảng) để thử lại lần khởi động sau.
pub async fn reconcile_startup(state: &SharedState) -> usize {
    let Some(db) = state.db.as_ref() else {
        return 0;
    };
    let persisted = db.load_running().unwrap_or_default();
    if persisted.is_empty() {
        return 0;
    }
    let live: std::collections::HashSet<u32> = match state.memuc.list_instances().await {
        Ok(v) => v.into_iter().map(|i| i.index).collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Reconcile khởi động: không liệt kê được VM — bỏ qua");
            return 0;
        }
    };
    let mut cleaned = 0;
    for (username, idx) in persisted {
        if live.contains(&idx) {
            tracing::warn!(
                username = %username,
                idx,
                "Reconcile: hủy VM mồ côi từ phiên trước (crash/tắt đột ngột)"
            );
            let _ = state.memuc.stop(idx).await;
            let _ = state.memuc.remove(idx).await;
            state.forget(idx).await;
            cleaned += 1;
        }
    }
    let _ = db.clear_running();
    cleaned
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
            state.running_profiles.lock().unwrap().len(),
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

    #[tokio::test]
    async fn reconcile_don_vm_mo_coi() {
        // Mô phỏng crash: persist một VM "đang chạy", xóa map bộ nhớ (như sau khởi động
        // lại), VM vẫn tồn tại trong memuc → reconcile phải HỦY nó + xóa bảng running.
        let state = make_state("recon", Arc::new(MockGeolocator));
        let idx = state.memuc.list_instances().await.unwrap()[0].index;
        state.set_running_profile("ghost", idx).await; // persist db + memory
        state.running_profiles.lock().unwrap().clear(); // mô phỏng mất memory sau restart
        let cleaned = reconcile_startup(&state).await;
        assert_eq!(cleaned, 1, "phải dọn 1 VM mồ côi");
        let live: Vec<u32> = state
            .memuc
            .list_instances()
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.index)
            .collect();
        assert!(!live.contains(&idx), "VM mồ côi đã bị hủy");
        assert!(
            state.db.as_ref().unwrap().load_running().unwrap().is_empty(),
            "bảng running đã được dọn"
        );
    }

    #[tokio::test]
    async fn run_loi_apk_guard_nha_cho_dat_truoc() {
        // run() lỗi ở fail-fast APK (sau khi đã đặt chỗ) → RAII guard PHẢI nhả chỗ
        // RESERVED, không rò slot cổng tối đa.
        let state = make_state("apkfail", Arc::new(MockGeolocator));
        create(&state, acc("acc_x"), None, None).await.unwrap();
        state.settings.lock().await.tiktok_apk_path = Some("Z:/khong/ton/tai.apk".into());
        let r = run(&state, "acc_x").await;
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "APK thiếu → lỗi");
        assert!(!state.is_reserved("acc_x").await, "guard phải nhả RESERVED");
        assert_eq!(
            state.running_profiles.lock().unwrap().len(),
            0,
            "không rò slot sau lỗi"
        );
    }
}
