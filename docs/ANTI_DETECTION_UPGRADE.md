# Nâng cấp chống-phát-hiện cho MPM (MuMu x86 chạy TikTok)

> Tổng hợp deep-research (106 agent, verify đối kháng 3-phiếu) + đối chiếu với các
> phát hiện chạy-thật của MPM. Mọi claim dưới đây ở mức **high-confidence, có nguồn**.

> **⚠️ TRẦN CHỐNG-PHÁT-HIỆN (đọc trước mọi thứ):** Trên MuMu **x86**, hai tell
> **native-bridge** (ARM→x86) và **hypervisor flag** là **CỐ HỮU** — không công cụ nào
> (kể cả Redroid-x86) gỡ được → **KHÔNG THỂ** đạt "máy ảo không thể phát hiện". MPM chỉ
> giảm bề mặt lộ ở các trường **SỬA ĐƯỢC**: độ phân giải/DPI, MAC, ẩn root.
>
> ✅ **`ro.product.model`** (MuMu random khi boot) và **`ro.build.characteristics=tablet`**
> nay **KHÓA ĐƯỢC RUNTIME** qua **Magisk resetprop
> standalone**: MPM trích `libmagisk.so` từ **Magisk APK** (ưu tiên file bạn trỏ trong Cài đặt,
> fallback sang `Magisk-v30.7.apk` đi kèm bản đóng gói), đẩy vào
> mỗi VM (đã có root native), chạy `magisk resetprop` — **không cần base image / cài Magisk
> hệ thống**. `lock_device_identity` sinh script + VERIFY (đọc lại model, build fingerprint,
> và `ro.build.characteristics` không còn là `tablet`), đã kiểm chứng thực khóa được cả model **có khoảng trắng**
> ("Redmi Note 8"). Xem
> [`BASE_IMAGE_MAGISK_SETUP.md`](BASE_IMAGE_MAGISK_SETUP.md). Để trống ô Magisk APK = dùng bản đi kèm
> nếu có; chỉ khi không có APK hợp lệ thì model không khóa được.
>
> ✅ **Resolution/DPI runtime** nay áp 2 lớp: `MuMuManager simulation custom_resolution` trước boot
> và `adb shell wm size/density` sau `wait_boot_completed`; MPM re-assert lại trước khi trả VM cho
> TikTok nếu vừa cài APK hoặc restore snapshot.
>
> ⚠️ Riêng **`android_id`** vẫn bị GMS ghi đè SAU khi cài+chạy TikTok (Android 8+ cấp id
> theo app) — MPM re-apply trước khi app chạy, nhưng đây vẫn là known-gap SSAID còn lại.
>
> Cổng "quốc gia yêu cầu" chỉ kiểm **IP thoát của HOST** (mọi VM chung NAT host) — là
> phép kiểm VPN mức-host, KHÔNG cách ly geo per-account. Chạy nhiều account cùng 1 IP
> vẫn là rủi ro liên-kết-thiết-bị lớn nhất còn lại.

## 1. Bức tranh tell còn lộ & bản chất

| Tell | Bản chất | Che được không? |
|------|----------|-----------------|
| **native-bridge** (`libnb.so`, `ro.dalvik.vm.native.bridge`) | Houdini — lớp dịch ARM→x86 của Intel/Google. **Bất kỳ image x86 chạy app ARM đều BẮT BUỘC lộ** | ❌ Không có tool OSS che ở tầng hệ thống. Chỉ Frida hook per-app (dễ bị dò ngược). Muốn hết → **ARM image/redroid-ARM** |
| **cờ `hypervisor`** trong `/proc/cpuinfo` | Chạy trong VM | ❌ Không có tool OSS strip khỏi nội dung cpuinfo trên x86 |
| **`ro.product.model` bị MuMu random khi boot** | MuMu ghi đè MUỘN trong boot (sau post-fs-data) | ✅ **resetprop re-apply mỗi boot** (khả thi, ABI-independent) |
| **`ro.build.characteristics=tablet`** | MuMu báo tablet trong khi profile giả là điện thoại Samsung/Redmi | ✅ **resetprop runtime**: profile có giá trị thật thì set, rỗng thì `--delete` prop để gỡ tell `tablet`; không dùng sed `/system/build.prop` vì VM disposable không reboot |
| imei / root | MPM áp qua `MuMuManager.exe simulation` trước khi chạy app | ✅ Đã ổn |
| độ phân giải/DPI runtime | MPM áp qua `custom_resolution`, sau boot gọi thêm `wm size` / `wm density`, và re-assert sau install/restore | ✅ Có mitigation; cần verify B2.1 trên máy thật |
| android_id | MPM áp/re-apply qua adb trước khi start app, nhưng Android 8+/GMS có thể cấp lại SSAID theo app sau khi cài/chạy TikTok | ⚠️ Known-gap, cần verify sau mỗi phiên |

**Kết luận cốt lõi:** native-bridge + hypervisor là rủi ro **cố hữu của x86**, không tool nào che triệt để ở tầng hệ thống. Thứ **sửa được ngay** là khóa `ro.product.model` + đồng bộ `ro.build.fingerprint`.

Nguồn Android chính: `ANDROID_ID` trên Android 8+ là giá trị scoped theo app-signing key/user/device
([Android docs](https://developer.android.com/reference/android/provider/Settings.Secure#ANDROID_ID)); AOSP
`SettingsProvider` đọc SSAID từ `settings_ssaid.xml` cho app UID thường, không phải luôn lấy trực tiếp
`settings_secure.xml` ([source](https://android.googlesource.com/platform/frameworks/base/+/master/packages/SettingsProvider/src/com/android/providers/settings/SettingsProvider.java)).

## 2. Kỹ thuật KHÓA model + đồng bộ fingerprint (khả thi, ưu tiên cao)

- **`resetprop`** (lõi Magisk) ghi thẳng vào `prop_area`, **bypass property_service** → sửa được cả `ro.*` read-only tại runtime. ABI-independent → chạy trên x86.
  - Nguồn: [Magisk docs](https://topjohnwu.github.io/Magisk/details.html), [resetprop-rs](https://github.com/Enginex0/resetprop-rs)
- **KHÔNG persistent qua reboot** → phải re-apply **mỗi boot**, ở **cả `post-fs-data` VÀ `late_start service.d`** vì MuMu ghi đè muộn. (MPM inject mỗi lần launch → hợp cơ chế này.)
  - Nguồn: [MagiskHidePropsConf README](https://github.com/Magisk-Modules-Repo/MagiskHidePropsConf)
- **Đồng bộ trọn bộ**: `ro.product.model/brand/manufacturer/device/name`, `ro.build.fingerprint`, `ro.build.characteristics` (nếu có nguồn thật; rỗng = xóa tell `tablet`), `ro.build.version.security_patch` — phải khớp nhau (nhất quán nội bộ).
- **Dùng thiết bị THẬT ĐỜI CŨ**, KHÔNG dùng Pixel mới/beta (bị gate qua hardware attestation). Mẫu coherent: **Samsung Galaxy Note FE** — `SM-N935F` / brand `samsung` / device `gracerlte` / hardware `samsungexynos8890` / fingerprint `samsung/gracerltexx/gracerlte:8.0.0/R16NW/N935FXXS4BRK2:user/release-keys`.
  - Nguồn: [Anti-EmuDetector](https://github.com/u0pattern/Anti-EmuDetector), [PlayIntegrityFork](https://github.com/osm0sis/PlayIntegrityFork)

## 3. Cài Magisk vào base image MuMu

- **MagiskOnEmu** (GPL-3.0, Shell) hỗ trợ MuMu trực tiếp; hoặc **Magisk Delta** (khuyến nghị hơn, cài Magisk vào system partition cho emulator).
  - Nguồn: [MagiskOnEmu](https://github.com/code871/MagiskOnEmu)
- ⚠️ Có báo cáo **flaky riêng với MuMu** (MagiskHideProps không chạy, root chập chờn) — cần verify.

## 4. Play Integrity modules — KHÔNG hữu ích cho mục tiêu này

- PIF / PlayIntegrityFork chỉ spoof **process-scoped** trong tiến trình GMS DroidGuard, **KHÔNG** spoof fingerprint toàn cục cho TikTok đọc, **KHÔNG** che emulator, **KHÔNG** đạt `MEETS_VIRTUAL_INTEGRITY`. Yêu cầu Zygisk.
  - Nguồn: [PlayIntegrityFix README](https://github.com/KOWX712/PlayIntegrityFix)

## 4.1. Shamiko root-hide — chỉ là lớp tùy chọn

- Shamiko cần **full Magisk + Zygisk** trong base image. `magiskApkPath` của MPM hiện chỉ trích
  binary `magisk resetprop` standalone, không đủ để bật Shamiko.
- Cấu hình đúng: TikTok trong DenyList, **Enforce DenyList tắt**, reboot, rồi chạy A.15:
  `cargo test --lib e2e_real::a15_magisk_shamiko_root_hide_diagnostics -- --ignored --nocapture`.
- MPM không dùng bind-mount global để đè `su`, vì cách đó dễ làm hỏng pipeline root của chính MPM
  và không tương đương ẩn root theo process.

## 5. Dự án tương tự

- **Waydroid** bake props (model + cả 3 fingerprint `ro.build/ro.system.build/ro.vendor.build.fingerprint`) vào `waydroid_base.prop` — **persistent, tiêu thụ lúc init** (cùng lớp với sed `/system/build.prop`, khác với setprop runtime bị reset).
  - Nguồn: [Waydroid-total-spoof](https://github.com/lil-xhris/Waydroid-total-spoof)

## 5.1. Sensor entropy — gap cần đo trước khi spoof

- Sensor không xử lý bằng `setprop`; app đọc qua SensorManager/native sensor APIs, nên cần event stream
  ở tầng framework/HAL nếu muốn làm chắc.
- MPM đã thêm diagnostic A.16 và `scan_emulator_tells` cho `Motion sensors`/`Sensor provider tells`.
- Hướng sạch nhất là Sensor HAL/host-to-guest bridge trong base image/custom image. Hook bằng
  Zygisk/LSPosed chỉ nên là phương án rủi ro cao vì tự thêm bề mặt bị dò.
- Chi tiết: [`SENSOR_ENTROPY_REMEDIATION.md`](SENSOR_ENTROPY_REMEDIATION.md).

---

## Kế hoạch tích hợp đề xuất cho MPM (phân tầng theo hiệu-quả/công-sức)

### 🟢 Tầng 1 — HIỆU QUẢ CAO / CÔNG SỨC THẤP (dùng root sẵn có, KHÔNG cần full Magisk)
1. **Bộ device coherent**: đổi `fingerprint::generate()` chọn theo **bộ thật đời cũ** (model+brand+manufacturer+device+`ro.build.fingerprint`+security_patch khớp nhau), thay vì model rời rạc. Thêm field `build_fingerprint` vào `HardwareProfile`.
2. **Khóa model post-boot bằng resetprop**: build resetprop từ nguồn (resetprop-rs, target x86_64) → push vào VM → sau `wait_boot_completed`, chạy `su -c 'resetprop ro.product.model … ; resetprop ro.build.fingerprint … ; …'` để **ghi đè lại giá trị MuMu random**. MPM re-apply sau boot và thêm một lần cuối sau install/restore.
3. **Khóa resolution runtime**: sau boot gọi `wm size <w>x<h>` + `wm density <dpi>`, verify bằng `wm size`/`wm density`; nếu MuMu vẫn trả `900x1600` thì ghi warn và fail ở bước kiểm B2.1. AOSP shell command hỗ trợ set forced display size/density qua WindowManager ([source](https://android.googlesource.com/platform/frameworks/base/+/master/services/core/java/com/android/server/wm/WindowManagerShellCommand.java)).
4. **Scan mở rộng**: `scan_emulator_tells` kiểm thêm `ro.product.model == kỳ vọng` + fingerprint nhất quán → cảnh báo nếu khóa thất bại.

### 🟡 Tầng 2 — TRUNG BÌNH (tùy chọn, chưa phải kiến trúc hiện tại)
4. Cài Magisk (Delta) vào base image → module boot script (post-fs-data + late_start) tự re-assert props; mở đường cho Zygisk/DenyList. Bản hiện tại ưu tiên cách standalone: trích binary từ Magisk APK rồi push vào VM disposable mỗi lần provision.

### 🔴 Tầng 3 — native-bridge / hypervisor (cố hữu x86)
5. **Chấp nhận rủi ro** (native-bridge + hypervisor luôn lộ trên MuMu), HOẶC
6. Chuyển sang **ARM image / redroid-ARM** để hết tell (thay đổi kiến trúc lớn; MuMu không làm ARM native).

### ⚠️ Lưu ý an toàn khi triển khai
- **KHÔNG** tải binary resetprop prebuilt từ repo lạ → **build từ nguồn** (auditable) hoặc lấy resetprop chính thức của Magisk.
- Frida/Zygisk **bản thân là vector phát hiện ngược** — cân nhắc kỹ.
- Fingerprint **chia sẻ công khai** dễ bị Google/ByteDance ban → dùng bộ curated riêng, xoay vòng.
