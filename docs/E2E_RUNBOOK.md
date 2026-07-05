# E2E RUNBOOK — Kiểm thử THẬT qua App Tauri + Đăng nhập TikTok thật

> Runbook thủ công (human-driven) cho phần mà bộ test tự động `e2e_real` cố tình **không** làm: đăng nhập TikTok bằng credential thật + xác nhận bằng mắt. Người vận hành lái app Tauri; máy không thể tự nhập OTP, không tự phán "còn đăng nhập không", không tự đánh giá cửa sổ/độ phân giải giả.

---

## 0. Nguyên tắc bất di bất dịch (đọc trước, vi phạm = STOP toàn bộ)

| # | Quy tắc | Vi phạm = |
|---|---------|-----------|
| R0.1 | **KHÔNG bao giờ đụng vào VM index 0** (VM đang chạy của người dùng). Mọi VM tạo trong runbook phải có index ≠ 0 và không nằm trong danh sách VM có sẵn trước khi bắt đầu. | FAIL cứng, dừng phiên |
| R0.2 | Mỗi VM thủ công tạo ra phải được **teardown (backup) hoặc remove** khi xong. Không để VM mồ côi. | FAIL vệ sinh |
| R0.3 | Chỉ được coi là PASS khi TikTok mở **đã đăng nhập sẵn** đúng tài khoản B.1, **không** hiện màn login. Mọi trạng thái mơ hồ = FAIL. | — |
| R0.4 | Ghi lại screenshot/log tại mỗi Checkpoint. Không có bằng chứng = coi như chưa PASS. | — |

---

## 1. Điều kiện tiên quyết (Preconditions) — cổng GO/NO-GO

Hoàn thành **toàn bộ** trước khi sang B.1. Bất kỳ dòng FAIL nào ⇒ **NO-GO**, không được bắt đầu.

| # | Kiểm tra | Cách xác minh | PASS | FAIL |
|---|----------|---------------|------|------|
| P1 | MEmu đã cài | `memuc.exe` tồn tại (mặc định `D:\Microvirt\MEmu\memuc.exe`) | File tồn tại, chạy được | Thiếu file / lỗi |
| P2 | APK TikTok có sẵn | `D:\MemuTiktok\appTiktok\tiktok-40-0-0.apk` tồn tại (~220MB) | File tồn tại, đúng kích thước | Thiếu / hỏng |
| P3 | App Tauri đã build & chạy | Mở được UI app | UI hiện, không crash | Không mở / crash |
| P4 | Có credential TikTok thật | Tài khoản + phương thức OTP (SMS/email) sẵn sàng | Đăng nhập tay được ngoài app | Không có |
| P5 | Chụp danh sách VM nền | Ghi lại tập index VM đang có (đặc biệt xác nhận **index 0 đang Running**) | Có danh sách `before`, index 0 Running | Không xác định được |
| P6 | (Khuyến nghị) Bộ test tự động đã xanh | `cargo test --lib e2e_real -- --ignored --nocapture` pass ở máy này | Các test A.* pass hoặc skip hợp lệ | Có test FAIL thật |

> **Ghi chú P5:** Ghi tập `before` (ví dụ `{0}`). Mọi VM tạo sau này phải có index **∉ before** và **≠ 0**. Dùng tập này làm mốc dọn dẹp cuối phiên: sau khi xong, tập index phải quay về đúng `before`.

---

## 2. B.1 — Provision + cài đặt + đăng nhập thật

**Mục tiêu:** Có một VM mới, TikTok đã cài, đăng nhập bằng credential thật, đến được feed "For You" với profile hiển thị.

| Bước | Hành động | Tiêu chí PASS | Tiêu chí FAIL |
|------|-----------|---------------|----------------|
| B1.1 | Trong app Tauri, tạo/provision một VM tài khoản, đặt nhãn `manual-login-1`. Chờ boot xong. | VM mới xuất hiện, index ∉ `before` & ≠ 0; boot hoàn tất (`sys.boot_completed == 1` nếu app phơi shell) | VM không tạo được / boot treo / index trùng VM cũ hoặc = 0 |
| B1.2 | Xác nhận TikTok đã cài (tự động; nếu chưa thì kích hoạt cài trong app). | `pm list packages` chứa `com.zhiliaoapp.musically`; hoặc icon TikTok hiện trong VM | Không thấy package / cài lỗi |
| B1.3 | Mở TikTok trong khung VM. **Đăng nhập bằng credential thật** (nhập OTP/SMS nếu cần). | Vào được feed "For You", thấy tab profile | Kẹt ở màn login / OTP không nhận / crash |
| B1.4 | **CHECKPOINT ĐĂNG NHẬP:** Ghi lại **username + avatar** đang hiển thị. Chụp màn hình. | Đã ghi username `__________`, có screenshot `B1-4.png` | Không ghi được trạng thái đăng nhập |

**PASS B.1:** cả 4 bước PASS, đặc biệt B1.4 xác nhận đăng nhập THÀNH CÔNG với tài khoản cụ thể (đây là "payload" cho B.3).

---

## 3. B.2 — Kiểm tra độ phân giải giả / cửa sổ giả (fake-window)

**Mục tiêu:** Xác nhận cửa sổ emulator render theo **HardwareProfile giả tiêm vào**, không phải mặc định của MEmu — trên phiên **đã đăng nhập**.

| Bước | Hành động | Tiêu chí PASS | Tiêu chí FAIL |
|------|-----------|---------------|----------------|
| B2.1 | Kiểm tra độ phân giải: Settings → Display trong Android, hoặc `wm size` / `wm density` qua shell nếu app phơi ra. | `wm size` khớp profile (vd `1080x1920`); `wm density` khớp (vd `320`) — **không** phải mặc định host-window của MEmu | Res/dpi là mặc định MEmu, không khớp profile |
| B2.2 | Quan sát bằng mắt bố cục UI TikTok. | UI phone dọc (portrait), **không** letterbox, chrome vừa khít thiết bị giả | Có viền đen/letterbox / tỉ lệ lệch / trông như tablet |
| B2.3 | Kiểm tra model giả: `getprop ro.product.model` (hoặc mục device-info bất kỳ). | Trả về model giả (vd `FRD-L19`) — fingerprint app nhìn thấy được | Trả về model MEmu thật / rỗng |

**PASS B.2:** cả 3 khớp profile; đánh giá bằng mắt trên session **đã login** (tương ứng A.4/A.8 nhưng do người phán).

> Đây là hạng mục **máy không tự làm được**: phán đoán trực quan cửa sổ/độ phân giải.

---

## 4. B.3 — PHIÊN TIKTOK SỐNG QUA backup → teardown → restore  ★ HẠNG MỤC CỐT LÕI ★

**Đây là phép thử sống-còn.** Analog thủ công của test tự động A.9, nhưng payload là **phiên TikTok thật** (cookie/prefs/databases), không phải marker tổng hợp. PASS ở đây = chứng minh `shared_prefs / databases / files / app_webview` round-trip qua backup→hủy VM→restore với đúng owner + SELinux context (chown -R / restorecon -R trong `RealAdbWorker::restore`), tức app **đọc lại được** phiên.

### 4.1 Trình tự bắt buộc (đúng thứ tự)

| Bước | Hành động | Tiêu chí PASS | Tiêu chí FAIL |
|------|-----------|---------------|----------------|
| B3.1 | Với tài khoản **đang đăng nhập** (từ B.1), kích hoạt **backup rồi teardown** trong app (backup xong, hủy VM). Đây là đường `teardown` = backup → destroy mà A.9 tự động hoá. | App báo backup THÀNH CÔNG **trước**; snapshot được ghi (có record trong DB); rồi VM bị hủy | Backup lỗi / app hủy VM mà chưa backup xong |
| B3.2 | Xác nhận VM đã bị hủy. | VM biến mất khỏi danh sách VM trong app; index đó không còn `list_instances` | VM vẫn còn / hủy nửa vời |
| B3.3 | **R-15 kiểm chứng thủ công:** xác nhận snapshot đã được ghi TRƯỚC khi hủy (nếu backup lỗi thì VM lẽ ra KHÔNG bị hủy). | Có snapshot mới nhất cho tài khoản này (verify sha256 pass); thứ tự backup-trước-hủy đúng | Không có snapshot nhưng VM vẫn bị hủy (vi phạm R-15) |
| B3.4 | Kích hoạt **launch/provision lại đúng tài khoản** (restore snapshot mới nhất vào VM mới) qua app. | VM mới tạo, index ∉ `before` & ≠ 0 & ≠ index cũ; restore chạy (verify → get → restore); boot xong | Provision lỗi / restore lỗi / verify sha256 FAIL |
| B3.5 | Mở TikTok. **KHÔNG nhập lại credential.** | (xem điều kiện PASS bên dưới) | (xem điều kiện FAIL bên dưới) |

### 4.2 Điều kiện phán quyết cuối (B3.5) — TIÊU CHÍ QUYẾT ĐỊNH

**PASS ⇔ TẤT CẢ:**
- TikTok mở ra **đã đăng nhập sẵn** — **KHÔNG** hiện màn login/OTP.
- **Cùng username + avatar** đúng như đã ghi ở B1.4.
- Không cần thao tác đăng nhập nào.

**FAIL ⇔ BẤT KỲ:**
- Hiện màn login/đăng ký, hoặc yêu cầu OTP lại.
- Đăng nhập nhầm tài khoản khác / trạng thái trống.
- TikTok crash khi mở phiên restore.

### 4.3 Quy trình chẩn đoán khi FAIL ở B3.5

Nếu hiện màn login (FAIL), thu thập theo thứ tự:
1. **Log app + log restore:** xác nhận restore có thực sự chạy không, `store.verify(storage_key, sha256)` có pass không.
2. **Owner + SELinux label** trên `/data/data/com.zhiliaoapp.musically`:
   - Owner mong đợi: `uXXX_aXXX` (dạng `stat -c %U:%G`).
   - SELinux label mong đợi: chứa `app_data_file` (`ls -Z`).
   - Label/owner sai ⇒ EACCES ⇒ app coi như **không có phiên** ⇒ bắt đăng nhập lại. Đây là nguyên nhân gốc phổ biến nhất.
3. Nếu verify sha256 FAIL ⇒ blob hỏng/không toàn vẹn ⇒ vấn đề backup/storage, không phải restore.
4. Ghi kết luận: (a) restore không chạy, (b) verify fail, (c) owner/label sai, hay (d) app-side đọc session lỗi.

**PASS B.3 (cốt lõi):** B3.1–B3.4 PASS và **B3.5 PASS** (đã đăng nhập sẵn, đúng tài khoản, không login lại).

> Đây là hạng mục **máy không tự phán được**: verdict "còn đăng nhập không" do người quyết.

---

## 5. B.4 — (Tùy chọn) Swap giữa hai tài khoản sống (R-12 isolation)

Chỉ chạy nếu cần kiểm đa tài khoản. Xác nhận thủ công của A.8 trên phiên sống.

**Tiền đề:** Tài khoản A đã qua B.1–B.3 (có snapshot login). Tài khoản B có snapshot đã-đăng-nhập riêng từ trước.

| Bước | Hành động | Tiêu chí PASS | Tiêu chí FAIL |
|------|-----------|---------------|----------------|
| B4.1 | Đăng nhập / khôi phục tài khoản A (theo B.1–B.3). | A mở đã đăng nhập, đúng A | A không login được |
| B4.2 | Trong app, thực hiện **swap_account** sang tài khoản B (B phải có snapshot login trước). | Swap chạy (wipe → apply hw → reboot → wait_boot → apply android_id → restore B → start_app) không lỗi | Swap lỗi |
| B4.3 | Mở TikTok sau swap. **Không nhập credential.** | TikTok đăng nhập là **B**, **KHÔNG** phải A; không thấy dữ liệu A rò rỉ | Vẫn là A / trạng thái trống / hỏi login |
| B4.4 | Kiểm fingerprint đã đổi: `ro.product.model` + `android_id` khác so với A. | Model/android_id là của profile B (khác A) | Fingerprint vẫn của A (rò rỉ) |

**PASS B.4:** B mở đúng là B, fingerprint đã đổi sang profile B — xác nhận R-12 isolation trên phiên sống, không rò rỉ chéo A↔B.

---

## 6. Bảng theo dõi kết quả (điền khi chạy)

**Thông tin phiên chạy**

| Trường | Giá trị |
|--------|---------|
| Ngày / giờ | `______________` |
| Người vận hành | `______________` |
| Máy / OS | `______________` |
| Phiên bản app Tauri | `______________` |
| Tập VM `before` (P5) | `______________` |
| Tài khoản A (username) | `______________` |
| Tài khoản B (nếu B.4) | `______________` |

**Preconditions**

| # | Hạng mục | Kết quả (PASS/FAIL/N/A) | Bằng chứng / Ghi chú |
|---|----------|--------------------------|----------------------|
| P1 | MEmu đã cài | | |
| P2 | APK TikTok có sẵn | | |
| P3 | App Tauri chạy | | |
| P4 | Credential TikTok thật | | |
| P5 | Danh sách VM nền (index 0 Running) | | |
| P6 | Test tự động xanh (khuyến nghị) | | |
| — | **GO / NO-GO** | | |

**B.1 — Provision + cài + login**

| Bước | Kết quả | Bằng chứng / Ghi chú |
|------|---------|----------------------|
| B1.1 Provision + boot (index = `____`) | | |
| B1.2 TikTok đã cài | | |
| B1.3 Đăng nhập thật đến feed | | |
| B1.4 Checkpoint username=`______` + screenshot | | |
| **Tổng B.1** | | |

**B.2 — Fake resolution / window**

| Bước | Kết quả | Giá trị đo được / Ghi chú |
|------|---------|---------------------------|
| B2.1 `wm size`/`wm density` khớp profile | | size=`____` dpi=`____` |
| B2.2 UI portrait, không letterbox | | |
| B2.3 `ro.product.model` = model giả | | model=`____` |
| **Tổng B.2** | | |

**B.3 — Phiên sống qua backup → teardown → restore ★**

| Bước | Kết quả | Bằng chứng / Ghi chú |
|------|---------|----------------------|
| B3.1 Backup THÀNH CÔNG trước, rồi teardown | | snapshot key=`____` |
| B3.2 VM đã hủy (biến khỏi list) | | |
| B3.3 R-15: snapshot ghi trước khi hủy (verify sha256 pass) | | sha256 ok? `____` |
| B3.4 Provision lại + restore vào VM mới (index=`____`) | | verify→get→restore ok? |
| **B3.5 VERDICT: TikTok đã đăng nhập sẵn, đúng tài khoản, không login lại** | | screenshot `B3-5.png`; username khớp B1.4? |
| Nếu FAIL: owner=`____` SELinux label=`____` verify=`____` nguyên nhân=`____` | | |
| **Tổng B.3 (CỐT LÕI)** | | |

**B.4 — Swap 2 tài khoản sống (tùy chọn)**

| Bước | Kết quả | Bằng chứng / Ghi chú |
|------|---------|----------------------|
| B4.1 A login đúng | | |
| B4.2 swap_account sang B không lỗi | | |
| B4.3 TikTok đăng nhập đúng B (không phải A) | | |
| B4.4 Fingerprint đổi sang B (model/android_id khác A) | | model=`____` android_id=`____` |
| **Tổng B.4** | | |

**Dọn dẹp cuối phiên (R0.2)**

| # | Hạng mục | Kết quả | Ghi chú |
|---|----------|---------|---------|
| C1 | Mọi VM thủ công đã teardown/remove | | |
| C2 | Tập index VM == `before` (index 0 còn nguyên, Running) | | |
| C3 | Không còn VM mồ côi | | |

**Kết luận tổng**

| Trường | Giá trị |
|--------|---------|
| B.1 | PASS / FAIL |
| B.2 | PASS / FAIL |
| **B.3 (cốt lõi)** | **PASS / FAIL** |
| B.4 (nếu chạy) | PASS / FAIL / N/A |
| Dọn dẹp | PASS / FAIL |
| **VERDICT TOÀN RUNBOOK** (PASS ⇔ B.1, B.2, B.3 đều PASS + dọn dẹp sạch) | |

---

## 7. Phụ lục — Tham chiếu nhanh

**Hằng số & đường dẫn**
- `TIKTOK_PKG = com.zhiliaoapp.musically`
- `DEFAULT_TIKTOK_APK = D:\MemuTiktok\appTiktok\tiktok-40-0-0.apk`
- `memuc.exe` mặc định: `D:\Microvirt\MEmu\memuc.exe`

**Thư mục được backup** (chỉ những cái này round-trip; phiên TikTok nằm trong đây): `shared_prefs`, `databases`, `files`, `app_webview` dưới `/data/data/com.zhiliaoapp.musically`.

**Owner / SELinux mong đợi sau restore** (khi B3.5 FAIL thì kiểm mục này đầu tiên):
- Owner: `uXXX_aXXX` (dạng `stat -c %U:%G`).
- SELinux label: chứa `app_data_file` (`ls -Z`).
- Sai owner/label ⇒ EACCES ⇒ app coi như mất phiên ⇒ bắt đăng nhập lại.

**Ánh xạ sang test tự động** (phần này bổ sung cho, không thay thế): B.2↔A.4/A.8 (fingerprint); **B.3↔A.9** (round-trip data-survival, nhưng payload là phiên thật); B.3.3↔A.10 (R-15 backup-trước-hủy); B.4↔A.8 (R-12 isolation).

**Điều máy KHÔNG tự làm được (lý do tồn tại runbook này):** nhập credential thật (B1.3), phán đoán trực quan cửa sổ/độ phân giải giả (B.2), và verdict "còn đăng nhập không" trên phiên restore (B3.5). Mọi bước còn lại đều phản chiếu các luồng orchestrator đã được tự động hoá.

---

## 8. E2E vòng đời PROFILE (A.12) — phát hiện qua chạy thật

`A.12 (a12_profile_lifecycle_real)` tự động hoá TOÀN BỘ vòng đời profile-centric qua
đúng code production `crate::profile_ops` (create → run → stop): create profile (KHÔNG
tạo VM) → run (provision VM sạch + fingerprint + **cài TikTok** + đăng ký running) →
run lần 2 idempotent → stop (backup phiên + **HỦY VM**) → profile bền + snapshot ghi DB
+ VM biến mất. Chạy: `cargo test --lib e2e_real::a12 -- --ignored --nocapture` (~80s).

Ba phát hiện chỉ lộ khi chạy thật (mỗi cái bị lỗi silent phía trước che):

1. **MEmu KHÔNG hỗ trợ adb streamed install.** `adb install` mặc định dùng streamed →
   `adb: failed to install ...: Performing Streamed Install` (thất bại tức thì, KHÔNG
   push byte nào). **Fix: bắt buộc `--no-streaming`** (push-rồi-install; 230MB ~5–12s →
   `Success`). `RealAdbWorker::install_apk`.

2. **`adb install` báo kết quả ở OUTPUT, không phải exit code** — nhiều bản exit 0 dù in
   `Failure`/`failed to install`. **Fix: đọc output tìm `Success`** (bắt cả stderr), nếu
   không thấy → `Err`. Trước đây lỗi này bị nuốt → VM chạy nhưng KHÔNG có TikTok.

3. **`failed to read copy response` chớp nhoáng** ngay sau provision boot + áp
   android_id/debloat/harden (rớt kết nối lúc commit dù push xong). **Fix: thử lại tối đa
   3 lần, chờ VM lắng** (kiểm chứng: lần 2 ăn).

**Known-gap (KHÔNG assert cứng, chỉ cảnh báo):**
- **android_id bị GMS/MEmu ghi đè SAU khi cài + chạy TikTok.** android_id áp được & BỀN
  khi CHƯA cài app (A.4 PASS: đọc lại đúng giá trị áp). Nhưng Android 8+ cấp android_id
  theo app và GMS tự quản → sau khi cài+chạy TikTok, `settings get secure android_id`
  trả giá trị KHÁC giá trị áp. Cùng lớp với known-gap `ro.product.model` (MEmu random khi
  boot). Muốn khóa cứng cần **Magisk + resetprop / Xposed-module** — user đã bỏ hướng #2.
  Fingerprint thực sự áp được & bền trên MEmu vanilla: **độ phân giải/DPI, MAC, root**.

**An toàn VM:** provision nay NGUYÊN TỬ — mọi bước sau `create_vm` mà lỗi thì tự
`stop + remove + forget` VM vừa tạo (không rò "VM mồ côi"). Đã kiểm chứng: 4 lần a12 fail
liên tiếp trong lúc gỡ lỗi đều để `listvms` về đúng `0,MEmu` (0 VM mồ côi).