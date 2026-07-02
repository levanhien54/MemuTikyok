# THIẾT KẾ CƠ CHẾ BACKUP / RESTORE — "MÔI TRƯỜNG DÙNG MỘT LẦN"

**Trạng thái:** v0.2 — đã nghiên cứu & xác minh dữ kiện kỹ thuật (nguồn ở §14). Hướng triển khai đã chốt: **Local-first + trừu tượng hóa để cắm server sau**.
**Phạm vi:** Cơ chế sao lưu (backup) dữ liệu phiên TikTok + hồ sơ phần cứng, và nạp lại (inject/restore) vào máy ảo mới, tối ưu dung lượng & tính toàn vẹn.
**Liên quan:** mở rộng từ [`kehoac.md`](../kehoac.md) và [`ARCHITECTURE.md`](./ARCHITECTURE.md).
**Phần B (§14–§19)** chứa chi tiết kỹ thuật đã xác minh: runbook backup/restore, schema local-first, trait Rust, checklist môi trường.

---

## 1. ĐÁNH GIÁ KIẾN TRÚC ĐỀ XUẤT

Triết lý **Disposable Environment** (server giữ dữ liệu, máy trạm chỉ cấp phát tạm) là **đúng đắn và mạnh** cho bài toán vận hành nhiều tài khoản:

**Ưu điểm:**
- Máy trạm luôn "sạch" → giảm rò rỉ/chồng chéo dữ liệu giữa các phiên (đúng như mục tiêu).
- Nguồn sự thật tập trung → dễ backup, dời máy, mở rộng số worker.
- Tách bạch **dữ liệu động** (session) khỏi **danh tính thiết bị** (hardware) — nền tảng để chống trùng lặp & giữ ổn định.

**Rủi ro/điểm phải giải quyết (chi tiết ở dưới):**
1. **Tính nhất quán fingerprint** — mỗi phiên phải tái tạo **ĐÚNG** hồ sơ phần cứng cũ, không random lại (nếu không → TikTok risk-engine gắn cờ/ban). Đây là điểm sống còn, không chỉ là "tạo máy mới".
2. **Khớp phiên bản APK** giữa lúc backup và lúc restore (schema DB nội bộ app).
3. **Quyền sở hữu file & SELinux** khi nạp `/data/data` (UID đổi theo mỗi lần cài → phải chown + restorecon).
4. **Toàn vẹn & nguyên tử** — chỉ hủy VM sau khi đã đồng bộ + verify checksum thành công.

> Kết luận: kiến trúc giữ nguyên; tài liệu này bổ sung các cơ chế đảm bảo 4 điểm trên.

---

## 2. PHÂN LOẠI DỮ LIỆU (cái gì backup, cái gì là config)

| Loại | Nội dung | Nơi lưu | Tính chất |
| :-- | :-- | :-- | :-- |
| **Session data** (động) | Thư mục app TikTok: prefs, databases, cookies, tokens, login | Cloud Storage (archive nén) | Thay đổi mỗi phiên → backup sau mỗi phiên |
| **Hardware profile** (tĩnh) | model, brand, fingerprint, IMEI, android_id, MAC, resolution/DPI, timezone, locale | Central DB (bảng) | Cố định/1 lần → **áp y hệt** mỗi phiên |
| **Account** | username/email + credential (mã hóa), trạng thái, proxy gán | Central DB | Ít đổi |
| **Trạng thái phiên (lease)** | worker, vm_index, state machine | Central DB | Thời gian thực |

**TikTok — vị trí dữ liệu phiên (ĐÃ XÁC MINH — nguồn §14):**
- Package (bản global): `com.zhiliaoapp.musically` (biến thể: `com.ss.android.ugc.trill`; Douyin: `com.ss.android.ugc.aweme`).
- **Cookie phiên:** `/data/data/com.zhiliaoapp.musically/app_webview/Default/Cookies` (SQLite; `/data/user/0/...` là cùng đường dẫn).
- **Databases:** `/data/data/<pkg>/databases/` (~28 SQLite; gồm `Cookies`, `WebData`).
- **Shared prefs:** `/data/data/<pkg>/shared_prefs/` (XML — flag đăng nhập, cấu hình).
- **files/**: dữ liệu nội bộ app.
- **LOẠI BỎ (rất lớn, không cần):** `cache/cachev2` (cache video), `cache/`, `code_cache/`, `app_webview/*/GPUCache/`, `/sdcard/Android/data/<pkg>/`.
- Truy cập cần **root** (kho private app).
- **Danh tính đăng ký server-side** (device_id/install_id) TikTok sinh ở lần chạy đầu, gắn với fingerprint → nằm trong session data. Vì vậy **backup thư mục data + hardware profile cố định** = giữ đúng danh tính, không bị coi là thiết bị lạ.

---

## 3. VÒNG ĐỜI PHIÊN (SESSION LIFECYCLE)

```mermaid
sequenceDiagram
    participant O as Orchestrator (máy trạm)
    participant DB as Central DB
    participant CS as Cloud Storage
    participant EMU as Emulator Controller (memuc)
    participant ADB as ADB Worker (root)

    O->>DB: Lease account (khóa, state=LEASED)
    DB-->>O: account + hardware_profile + snapshot mới nhất
    O->>EMU: Clone từ base image (TikTok cài sẵn, root ON)
    O->>EMU: Áp hardware profile (setconfigex + build.prop + android_id)
    O->>EMU: Start & chờ boot xong
    O->>CS: Tải snapshot (verify sha256)
    O->>ADB: RESTORE: force-stop app → giải nén vào /data/data → chown + restorecon
    O->>EMU: (App chạy nhiệm vụ)
    O->>ADB: BACKUP: force-stop app → tar data (pruned) → nén
    O->>CS: Upload archive + sha256 (state=SYNCING→SYNCED)
    O->>DB: Cập nhật snapshot latest, giải phóng lease
    O->>EMU: Stop & DESTROY VM (chỉ khi SYNCED)
```

**Bất biến an toàn:** VM **không bị hủy** cho tới khi archive đã upload & verify. Nếu worker chết giữa chừng → state DB (`LEASED/RESTORING/RUNNING/SYNCING/SYNCED`) cho phép phục hồi/không mất dữ liệu.

---

## 4. ADB WORKER — CHI TIẾT KỸ THUẬT

Yêu cầu **root** trong máy ảo (MEmu có toggle root). Mọi lệnh nhạy cảm qua `su -c`.

### 4.1. BACKUP (trích xuất)
1. **Dừng app sạch:** `am force-stop <pkg>` (đảm bảo flush dữ liệu ra đĩa). Chờ ngắn.
2. **Đóng gói giữ quyền, loại cache:**
   `su -c "tar --exclude='<pkg>/cache' --exclude='<pkg>/code_cache' -cf - -C /data/data <pkg>"`
   truyền ra host bằng `memuc adb -i <idx> exec-out ...` → nén **zstd** (khuyến nghị, tốt hơn gzip) → `snapshot.tar.zst`.
3. **Băm & kích thước:** tính `sha256`, ghi DB cùng `apk_version`, `created_at`.
4. **Chỉ đánh dấu SYNCED** sau khi upload + verify.

### 4.2. RESTORE (nạp)
1. **Đảm bảo cùng phiên bản APK** như snapshot (base image nên pin sẵn; hoặc cài đúng APK).
2. `am force-stop <pkg>`; (tuỳ chọn `pm clear <pkg>` để về sạch trước khi nạp).
3. **Xác định UID mới** của app: `stat -c '%u' /data/data/<pkg>` (hoặc `dumpsys package <pkg> | grep userId`).
4. **Giải nén** archive vào `/data/data/`.
5. **Sửa sở hữu & nhãn bảo mật (bắt buộc):**
   `su -c "chown -R <uid>:<uid> /data/data/<pkg> && restorecon -R /data/data/<pkg>"`
   → nếu bỏ bước này app sẽ crash / đăng xuất (lỗi phổ biến nhất khi restore /data/data).
6. Start app.

> **Vì sao không dùng `adb backup/restore`:** đã deprecated, nhiều app opt-out (`allowBackup=false`), không lấy được đầy đủ → dùng `tar` qua root là chuẩn tin cậy.

### 4.3. Trừu tượng hóa (khớp codebase hiện tại)
Thêm module `adb/` với trait — cùng pattern adapter như `MemucClient`/`IpGeolocator`:
```rust
#[async_trait]
pub trait AdbWorker: Send + Sync {
    async fn backup(&self, idx: u32, pkg: &str, dst: &Path) -> AppResult<BackupMeta>; // sha256,size,apk_ver
    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> AppResult<()>;
    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()>;
}
```
→ có `RealAdbWorker` (gọi `memuc adb`) và `MockAdbWorker` (test không cần MEmu).

---

## 5. TỐI ƯU LƯU TRỮ

| Kỹ thuật | Lợi ích | Ghi chú |
| :-- | :-- | :-- |
| **Loại cache/GPUCache/log** khi tar | Giảm 60–90% dung lượng | Whitelist thư mục cần: `shared_prefs`, `databases`, `files`, `app_webview` (trừ GPUCache) |
| **Nén zstd** (`--long`) | Tỉ lệ nén tốt + nhanh hơn gzip | Giữ tương thích `.tar.gz` nếu cần |
| **Dedup theo nội dung (content-addressed)** | Nhiều snapshot chung file → chỉ lưu 1 lần | Lưu blob theo sha256; snapshot = danh sách blob |
| **Delta/incremental** | Chỉ lưu phần thay đổi so với snapshot trước | Cân nhắc; full-snapshot-đã-prune thường chỉ vài–vài chục MB nên có thể để sau |
| **Versioning + retention** | Giữ N snapshot/tài khoản để rollback | Đánh dấu `is_latest`; xóa cũ theo chính sách |

**Khuyến nghị v1:** full-snapshot đã prune + nén zstd + dedup blob ở tầng storage. Delta để Giai đoạn sau (phức tạp/độ lợi biên thấp khi data đã nhỏ).

---

## 6. HỒ SƠ PHẦN CỨNG — LƯU & ÁP DỤNG NHẤT QUÁN

**Trường (Central DB, ràng buộc UNIQUE để chống trùng):** model, brand, device, manufacturer, `ro.build.fingerprint`, IMEI, `android_id`, MAC, resolution, DPI, timezone, locale, proxy_ref.

**Áp dụng mỗi phiên (idempotent, luôn ra cùng kết quả):**
- Qua `memuc setconfigex`: imei, model, manufacturer, brand, phone number, resolution/dpi, MAC…
- `ro.*` (fingerprint/model) không set runtime được → dùng cấu hình MEmu hoặc sửa `build.prop` trong base image.
- `android_id`: `su -c "settings put secure android_id <val>"`.
- timezone/locale: qua `setprop`/`settings`.

> **Nguyên tắc vàng:** cùng một tài khoản → **luôn** cùng một fingerprint. Central DB đảm bảo không hai tài khoản trùng bộ định danh (tránh liên kết chéo/ban dây chuyền).

---

## 7. TOÀN VẸN & AN TOÀN

- **Checksum sha256** cho mỗi archive; verify **sau khi tải về, trước khi restore**.
- **Nguyên tử phía server:** upload lên key tạm rồi đổi `is_latest` sau khi hoàn tất (không ghi đè bản tốt bằng bản hỏng).
- **State machine** trong DB để phục hồi sau sự cố; **không hủy VM** khi chưa `SYNCED`.
- **Kiểm tra rỗng/không hợp lệ**: từ chối archive quá nhỏ/không giải nén được.
- **Khớp APK version**: từ chối restore nếu version lệch (hoặc migrate có kiểm soát).

---

## 8. BẢO MẬT

- Archive = **chứa token/cookie đăng nhập** → coi như bí mật: **mã hóa at-rest** (server) + **TLS in-transit**; khóa quản lý riêng.
- Credential tài khoản mã hóa (§9 SEC-3 SRS).
- Máy trạm: xóa sạch archive tạm + VM sau phiên (đúng triết lý disposable).
- Log không chứa token/cookie/credential.

---

## 9. LƯỢC ĐỒ CENTRAL DB (server — vd PostgreSQL)

```
accounts(id, username, email, cred_enc, status, note, created_at)
hardware_profiles(account_id FK UNIQUE, model, brand, device, manufacturer,
                  fingerprint, imei UNIQUE, android_id UNIQUE, mac UNIQUE,
                  resolution, dpi, timezone, locale, proxy_id FK)
snapshots(id, account_id FK, storage_key, sha256, size, apk_version,
          created_at, is_latest)
leases(id, account_id FK, worker_id, vm_index, state, started_at, heartbeat_at)
proxies(id, type, host, port, user_enc, pass_enc, sticky_ip, country)
```
Cloud layout: `storage://bucket/accounts/<id>/snapshots/<ts>.tar.zst` (+ dedup blob store nếu dùng).

---

## 10. TÍCH HỢP VÀO CODEBASE HIỆN TẠI

| Thành phần đề xuất | Ánh xạ vào dự án |
| :-- | :-- |
| Emulator Controller | **Đã có** — trait `MemucClient` (`src-tauri/src/memuc/`) |
| ADB Worker | **Thêm mới** — module `adb/` + trait `AdbWorker` (real/mock) |
| Central DB | Hiện là **SQLite cục bộ** (`db.rs`) → trừu tượng hóa sau trait `Repository`; bản server-centric thay bằng client gọi API máy chủ |
| Cloud Storage | **Thêm** — trait `SnapshotStore` (local FS / S3-compatible) |
| Orchestrator | **Thêm** — state machine điều phối create→apply→restore→run→backup→sync→destroy |
| Geolocation/Proxy | Gắn với `proxies` + cột Quốc gia đã làm (IP thực ⇄ proxy sticky) |

**Quyết định kiến trúc quan trọng — Local-first vs Server-centric:**
- Đề xuất của bạn là **server-centric** (nhiều worker, CSDL + Cloud trên server).
- Khuyến nghị: **trừu tượng hóa `Repository` + `SnapshotStore`** ngay từ đầu; cho phép chạy **local-first** (SQLite + thư mục local) để phát triển/1 máy, và **cắm server** (Postgres + S3 + API) khi mở rộng — **không phải viết lại**.

---

## 11. RỦI RO BỔ SUNG (nối tiếp Risk Register §15 SRS)

| Mã | Rủi ro | A.hưởng | Giải pháp |
| :-- | :-- | :--: | :-- |
| R-12 | Fingerprint không nhất quán giữa các phiên → account bị gắn cờ/ban | Cao | Pin hồ sơ/ tài khoản; áp y hệt mỗi phiên; verify trước khi chạy |
| R-13 | Lệch phiên bản APK backup↔restore → hỏng dữ liệu app | Cao | Pin apk_version theo snapshot; base image cố định version |
| R-14 | Sai UID/SELinux khi restore → app crash/logout | Cao | chown theo UID mới + `restorecon -R` (bắt buộc) |
| R-15 | Hủy VM khi chưa đồng bộ xong → mất session | Cao | State machine; chỉ destroy khi `SYNCED` |
| R-16 | Archive hỏng khi truyền | T.bình | sha256 verify trước restore; nguyên tử `is_latest` |
| R-17 | Trùng IMEI/android_id giữa tài khoản | Cao | Ràng buộc UNIQUE ở Central DB |
| R-18 | Root/`allowBackup=false` chặn thao tác | T.bình | Dùng `su -c tar` (không phụ thuộc adb backup) |
| R-19 | Tranh chấp lease (2 worker cùng account) | T.bình | Khóa lease + heartbeat trong DB |

---

## 12. CÂU HỎI MỞ CẦN CHỐT (trước khi triển khai)

1. **Local-first hay Server-centric ngay?** (quyết định có dựng server API/Postgres/S3 hay trừu tượng hóa để làm local trước).
2. **Biến thể & phiên bản TikTok** đích? (`com.zhiliaoapp.musically`?) Có cố định 1 APK version không?
3. **Root trên MEmu**: image nền đã bật root sẵn chưa? Có base image cài sẵn TikTok không?
4. **Quy mô**: vẫn tối đa 5 VM/máy trạm? Bao nhiêu tài khoản tổng? Bao nhiêu máy trạm?
5. **Proxy**: mỗi tài khoản gán proxy sticky (IP/quốc gia cố định) — có sẵn nguồn proxy chưa? (liên quan trực tiếp cột Quốc gia).
6. **Cloud Storage**: dùng S3-compatible (MinIO/AWS) hay thư mục mạng/local?

---

## 13. LỘ TRÌNH TRIỂN KHAI ĐỀ XUẤT

- **Spike (1)**: xác minh path dữ liệu TikTok + quy trình tar/chown/restorecon trên 1 máy ảo root thật; đo dung lượng trước/sau prune.
- **GĐ B1**: `AdbWorker` (backup/restore local) + `SnapshotStore` local + verify sha256. Test bằng mock.
- **GĐ B2**: Hardware profile apply nhất quán + ràng buộc unique (Repository).
- **GĐ B3**: Orchestrator state machine + disposable lifecycle (create→…→destroy) trên 1 máy (local-first).
- **GĐ B4**: Server-centric: Repository/SnapshotStore bản remote (API + Postgres + S3), lease/heartbeat, mã hóa.

---

# PHẦN B — CHI TIẾT KỸ THUẬT ĐÃ XÁC MINH (v0.2)

## 14. RUNBOOK BACKUP / RESTORE (spike-ready)

> Mọi lệnh gọi qua `memuc -i <idx> adb "shell ..."`. `<pkg>` = `com.zhiliaoapp.musically`.
> Yêu cầu root (`enable_su`). Tên biến `<uid>` = chủ sở hữu thư mục data (dạng `u0_aXX`).

### 14.1. BACKUP
```sh
# 1) Dừng app sạch để flush WAL/SQLite ra đĩa
memuc -i $IDX adb "shell su -c 'am force-stop $PKG'"

# 2) (Tuỳ chọn) checkpoint WAL để cookies/db nhất quán
#    -> đã force-stop là đủ trong đa số trường hợp

# 3) tar CHỈ các thư mục cần, loại cache; giữ quyền & context số
#    exec-out để lấy binary sạch (không bị mangle newline)
memuc -i $IDX adb "exec-out su -c 'cd /data/data/$PKG && \
  tar --exclude=cache --exclude=code_cache --exclude=app_webview/*/GPUCache \
      -cf - shared_prefs databases files app_webview'" > raw.tar

# 4) Nén zstd + băm
zstd -19 --long=27 -o snapshot.tar.zst raw.tar
sha256sum snapshot.tar.zst   # -> lưu DB cùng apk_version, created_at
```
> Chỉ đánh dấu `SYNCED` sau khi upload + verify sha256.

### 14.2. RESTORE
```sh
# 0) Base image đã cài ĐÚNG apk_version của snapshot (xem §6/R-13)

# 1) Dừng & (tuỳ chọn) xoá sạch trước khi nạp
memuc -i $IDX adb "shell su -c 'am force-stop $PKG'"
# pm clear $PKG   # nếu muốn về trạng thái trắng trước khi restore

# 2) Xác định UID hiện tại của app (đổi theo mỗi lần cài)
UID=$(memuc -i $IDX adb "shell su -c 'stat -c %U /data/data/$PKG'")   # vd u0_a123

# 3) Đẩy archive vào máy ảo và giải nén vào /data/data
#    (giải nén phía trong để giữ quyền)
memuc -i $IDX adb "push snapshot.tar.zst /data/local/tmp/"
memuc -i $IDX adb "shell su -c 'zstd -d /data/local/tmp/snapshot.tar.zst -o /data/local/tmp/s.tar && \
  tar -xf /data/local/tmp/s.tar -C /data/data/$PKG'"

# 4) BẮT BUỘC: sửa chủ sở hữu + nhãn SELinux (nếu thiếu -> app crash/logout)
memuc -i $IDX adb "shell su -c 'chown -R $UID:$UID /data/data/$PKG && restorecon -R /data/data/$PKG'"

# 5) Dọn tạm & khởi động
memuc -i $IDX adb "shell su -c 'rm /data/local/tmp/s.tar /data/local/tmp/snapshot.tar.zst'"
memuc -i $IDX adb "shell monkey -p $PKG 1"
```
> **Lỗi kinh điển:** quên `restorecon` → SQLite "unable to open database / not readable" → app tự đăng xuất. (Nguồn §14 SELinux.)

## 15. HỒ SƠ PHẦN CỨNG — ÁP DỤNG (khoá memuc ĐÃ XÁC MINH)

| Thuộc tính | Cách áp |
| :-- | :-- |
| IMEI | `memuc setconfigex -i $IDX imei <val>` |
| Model | `memuc setconfigex -i $IDX microvirt_vm_model <val>` (vd FRD-L19) |
| Manufacturer | `memuc setconfigex -i $IDX microvirt_vm_manufacturer <val>` |
| Brand | `memuc setconfigex -i $IDX microvirt_vm_brand <val>` |
| MAC | `memuc setconfigex -i $IDX macaddress <val>` |
| Độ phân giải/DPI | `memuc setconfigex -i $IDX custom_resolution <w> <h> <dpi>` |
| CPU / RAM | `memuc setconfigex -i $IDX cpus <n>` / `memory <MB>` |
| Root | `memuc setconfigex -i $IDX enable_su 1` |
| **android_id** | KHÔNG phải khoá memuc → `adb shell su -c 'settings put secure android_id <val>'` |
| **ro.build.fingerprint / ro.product.***| KHÔNG set runtime được → sửa `build.prop` trong **base image** (pin sẵn) |

> **Bất biến:** cùng account ⇒ cùng toàn bộ giá trị trên, áp **trước khi** app chạy lần đầu trong phiên. Central DB giữ UNIQUE cho imei/android_id/mac (chống trùng — R-17).

## 16. LỰC ĐỒ LOCAL-FIRST (SQLite) + ÁNH XẠ SERVER

Local dùng chính `db.rs` hiện có, thêm bảng. Khi lên server: cùng schema chuyển sang Postgres, `SnapshotStore` local→S3.
```sql
-- Mở rộng từ instance_meta hiện tại
accounts(id, username, email, cred_enc, hardware_profile_id, status, note, created_at);
hardware_profiles(id, model, brand, device, manufacturer, fingerprint,
                  imei UNIQUE, android_id UNIQUE, mac UNIQUE,
                  res_w, res_h, dpi, timezone, locale, proxy_id);
snapshots(id, account_id, storage_key, sha256, size_bytes, apk_version,
          created_at, is_latest);
leases(id, account_id, worker_id, vm_index, state, started_at, heartbeat_at);
proxies(id, type, host, port, user_enc, pass_enc, sticky_ip, country);
```

## 17. TRAIT RUST (biên giới trừu tượng — local↔server không đổi call-site)

```rust
// Backup/restore trong máy ảo (real: memuc adb; mock: test không cần MEmu)
#[async_trait] pub trait AdbWorker: Send + Sync {
    async fn backup(&self, idx: u32, pkg: &str, out: &Path) -> AppResult<SnapshotMeta>;
    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> AppResult<()>;
    async fn apply_hardware(&self, idx: u32, hw: &HardwareProfile) -> AppResult<()>;
}

// Kho snapshot (local FS ⇄ S3) — dedup theo sha256
#[async_trait] pub trait SnapshotStore: Send + Sync {
    async fn put(&self, key: &str, file: &Path) -> AppResult<()>;
    async fn get(&self, key: &str, dst: &Path) -> AppResult<()>;
    async fn verify(&self, key: &str, sha256: &str) -> AppResult<bool>;
}

// Nguồn sự thật dữ liệu (SQLite local ⇄ API server)
#[async_trait] pub trait Repository: Send + Sync {
    async fn lease_account(&self, worker: &str) -> AppResult<Option<LeasedAccount>>;
    async fn latest_snapshot(&self, account_id: i64) -> AppResult<Option<SnapshotMeta>>;
    async fn record_snapshot(&self, account_id: i64, meta: &SnapshotMeta) -> AppResult<()>;
    async fn set_lease_state(&self, lease_id: i64, state: LeaseState) -> AppResult<()>;
}
```
Orchestrator (state machine) điều phối: `Lease → Clone → ApplyHardware → Restore → Run → Backup → Sync → Destroy`, đảm bảo **không Destroy khi chưa Synced**.

## 18. CHECKLIST XÁC MINH MÔI TRƯỜNG (làm ở Spike trước khi code)

- [ ] MEmu image: `enable_su 1` hoạt động (`adb shell su -c id` ra uid=0)?
- [ ] Base image đã cài **đúng 1 phiên bản** TikTok? Ghi lại `apk_version` (`dumpsys package $PKG | grep versionName`).
- [ ] `zstd` có sẵn trong máy ảo? Nếu không → nén ở host thay vì trong VM (điều chỉnh runbook §14).
- [ ] `restorecon` có trong image? (một số ROM thiếu → cần `toybox restorecon` hoặc set context tay).
- [ ] Đo dung lượng data thật: trước/sau khi prune cache (ước tính snapshot MB).
- [ ] Proxy sticky: xác nhận IP/quốc gia cố định per-account (khớp cột Quốc gia đã làm).
- [ ] Thời gian một vòng lease→destroy (đánh giá thông lượng với tối đa 5 VM).

## 19. QUYẾT ĐỊNH ĐÃ CHỐT (từ trao đổi)
- Hướng: **Local-first**, trừu tượng `Repository`/`SnapshotStore`/`AdbWorker` để cắm server sau.
- Môi trường đích: MEmu **root ON**, **base image cài sẵn TikTok**, **proxy sticky per-account** (một số điểm còn cần xác minh — dùng Checklist §18).
- Bước kế: **nghiên cứu/thiết kế sâu** (tài liệu này) trước khi implement.

## 20. NGUỒN THAM KHẢO (đã tra cứu)
- TikTok Android forensic data paths (cookies `app_webview/Default/Cookies`, `databases/`, `shared_prefs/`, cache `cachev2`): [ACM post-mortem forensic artifacts of TikTok](https://dl.acm.org/doi/pdf/10.1145/3407023.3409203), [abrignoni — Finding TikTok messages in Android](https://abrignoni.blogspot.com/2018/11/finding-tiktok-messages-in-android.html)
- Restore ownership + SELinux: [Restoring SELinux Labels After Data Backup (newspaint)](https://newspaint.wordpress.com/2016/05/03/restoring-selinux-labels-after-restoring-from-data-backup-to-android/), [Android manually restoring apps from TWRP (semipol.de)](https://www.semipol.de/posts/2016/07/android-manually-restoring-apps-from-a-twrp-backup/), [AOSP Implement SELinux](https://source.android.com/docs/security/features/selinux/implement)
- memuc lệnh & setconfigex: [MEMUC Reference Manual (memuplay)](https://www.memuplay.com/blog/memucommand-reference-manual.html), [How to Manipulate MEmu thru Command Line](https://www.memuplay.com/blog/how-to-manipulate-memu-thru-command-line.html)
