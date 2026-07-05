# Nâng cấp chống-phát-hiện cho MPM (MEmu x86 chạy TikTok)

> Tổng hợp deep-research (106 agent, verify đối kháng 3-phiếu) + đối chiếu với các
> phát hiện chạy-thật của MPM. Mọi claim dưới đây ở mức **high-confidence, có nguồn**.

## 1. Bức tranh tell còn lộ & bản chất

| Tell | Bản chất | Che được không? |
|------|----------|-----------------|
| **native-bridge** (`libnb.so`, `ro.dalvik.vm.native.bridge`) | Houdini — lớp dịch ARM→x86 của Intel/Google. **Bất kỳ image x86 chạy app ARM đều BẮT BUỘC lộ** | ❌ Không có tool OSS che ở tầng hệ thống. Chỉ Frida hook per-app (dễ bị dò ngược). Muốn hết → **ARM image/redroid-ARM** |
| **cờ `hypervisor`** trong `/proc/cpuinfo` | Chạy trong VM | ❌ Không có tool OSS strip khỏi nội dung cpuinfo trên x86 |
| **`ro.product.model` bị MEmu random khi boot** | MEmu ghi đè MUỘN trong boot (sau post-fs-data) | ✅ **resetprop re-apply mỗi boot** (khả thi, ABI-independent) |
| android_id / imei / độ phân giải | (MPM đã inject, giữ được) | ✅ Đã ổn |

**Kết luận cốt lõi:** native-bridge + hypervisor là rủi ro **cố hữu của x86**, không tool nào che triệt để ở tầng hệ thống. Thứ **sửa được ngay** là khóa `ro.product.model` + đồng bộ `ro.build.fingerprint`.

## 2. Kỹ thuật KHÓA model + đồng bộ fingerprint (khả thi, ưu tiên cao)

- **`resetprop`** (lõi Magisk) ghi thẳng vào `prop_area`, **bypass property_service** → sửa được cả `ro.*` read-only tại runtime. ABI-independent → chạy trên x86.
  - Nguồn: [Magisk docs](https://topjohnwu.github.io/Magisk/details.html), [resetprop-rs](https://github.com/Enginex0/resetprop-rs)
- **KHÔNG persistent qua reboot** → phải re-apply **mỗi boot**, ở **cả `post-fs-data` VÀ `late_start service.d`** vì MEmu ghi đè muộn. (MPM inject mỗi lần launch → hợp cơ chế này.)
  - Nguồn: [MagiskHidePropsConf README](https://github.com/Magisk-Modules-Repo/MagiskHidePropsConf)
- **Đồng bộ trọn bộ**: `ro.product.model/brand/manufacturer/device/name`, `ro.build.fingerprint`, `ro.build.version.security_patch` — phải khớp nhau (nhất quán nội bộ).
- **Dùng thiết bị THẬT ĐỜI CŨ**, KHÔNG dùng Pixel mới/beta (bị gate qua hardware attestation). Mẫu coherent: **Samsung Galaxy Note FE** — `SM-N935F` / brand `samsung` / device `gracerlte` / hardware `samsungexynos8890` / fingerprint `samsung/gracerltexx/gracerlte:8.0.0/R16NW/N935FXXS4BRK2:user/release-keys`.
  - Nguồn: [Anti-EmuDetector](https://github.com/u0pattern/Anti-EmuDetector), [PlayIntegrityFork](https://github.com/osm0sis/PlayIntegrityFork)

## 3. Cài Magisk vào base image MEmu

- **MagiskOnEmu** (GPL-3.0, Shell) hỗ trợ MEmu trực tiếp; hoặc **Magisk Delta** (khuyến nghị hơn, cài Magisk vào system partition cho emulator).
  - Nguồn: [MagiskOnEmu](https://github.com/code871/MagiskOnEmu)
- ⚠️ Có báo cáo **flaky riêng với MEmu** (MagiskHideProps không chạy, root chập chờn) — cần verify.

## 4. Play Integrity modules — KHÔNG hữu ích cho mục tiêu này

- PIF / PlayIntegrityFork chỉ spoof **process-scoped** trong tiến trình GMS DroidGuard, **KHÔNG** spoof fingerprint toàn cục cho TikTok đọc, **KHÔNG** che emulator, **KHÔNG** đạt `MEETS_VIRTUAL_INTEGRITY`. Yêu cầu Zygisk.
  - Nguồn: [PlayIntegrityFix README](https://github.com/KOWX712/PlayIntegrityFix)

## 5. Dự án tương tự

- **Waydroid** bake props (model + cả 3 fingerprint `ro.build/ro.system.build/ro.vendor.build.fingerprint`) vào `waydroid_base.prop` — **persistent, tiêu thụ lúc init** (cùng lớp với sed `/system/build.prop`, khác với setprop runtime bị reset).
  - Nguồn: [Waydroid-total-spoof](https://github.com/lil-xhris/Waydroid-total-spoof)

---

## Kế hoạch tích hợp đề xuất cho MPM (phân tầng theo hiệu-quả/công-sức)

### 🟢 Tầng 1 — HIỆU QUẢ CAO / CÔNG SỨC THẤP (dùng root sẵn có, KHÔNG cần full Magisk)
1. **Bộ device coherent**: đổi `fingerprint::generate()` chọn theo **bộ thật đời cũ** (model+brand+manufacturer+device+`ro.build.fingerprint`+security_patch khớp nhau), thay vì model rời rạc. Thêm field `build_fingerprint` vào `HardwareProfile`.
2. **Khóa model post-boot bằng resetprop**: build resetprop từ nguồn (resetprop-rs, target x86_64) → push vào VM → sau `wait_boot_completed`, chạy `su -c 'resetprop ro.product.model … ; resetprop ro.build.fingerprint … ; …'` để **ghi đè lại giá trị MEmu random**. Re-apply mỗi launch (MPM đã làm vậy).
3. **Scan mở rộng**: `scan_emulator_tells` kiểm thêm `ro.product.model == kỳ vọng` + fingerprint nhất quán → cảnh báo nếu khóa thất bại.

### 🟡 Tầng 2 — TRUNG BÌNH (cần Magisk trong base image)
4. Cài Magisk (Delta) vào base image → module boot script (post-fs-data + late_start) tự re-assert props; mở đường cho Zygisk/DenyList.

### 🔴 Tầng 3 — native-bridge / hypervisor (cố hữu x86)
5. **Chấp nhận rủi ro** (native-bridge + hypervisor luôn lộ trên MEmu), HOẶC
6. Chuyển sang **ARM image / redroid-ARM** để hết tell (thay đổi kiến trúc lớn; MEmu không làm ARM native).

### ⚠️ Lưu ý an toàn khi triển khai
- **KHÔNG** tải binary resetprop prebuilt từ repo lạ → **build từ nguồn** (auditable) hoặc lấy resetprop chính thức của Magisk.
- Frida/Zygisk **bản thân là vector phát hiện ngược** — cân nhắc kỹ.
- Fingerprint **chia sẻ công khai** dễ bị Google/ByteDance ban → dùng bộ curated riêng, xoay vòng.
