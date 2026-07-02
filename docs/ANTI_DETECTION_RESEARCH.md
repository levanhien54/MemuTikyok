# NGHIÊN CỨU CHỐNG PHÁT HIỆN MÁY ẢO (TikTok anti-detection)

**Nguồn:** deep-research (21 nguồn · 96 claim → 25 kiểm chứng đối kháng · 21 xác nhận / 4 bác bỏ) + **scan thực tế trên MEmu của dự án**.
**Ngày:** 2026-07-02.

---

## 0. Kết luận cốt lõi
1. Phát hiện emulator là **đa tầng**. Với emulator **x86 (MEmu/LDPlayer)**, dấu vết **khó giấu nhất** là **Native Bridge ARM→x86** (`ro.dalvik.vm.native.bridge`→lib dịch, `persist.sys.nativebridge=1`) và cờ **`hypervisor`** trong `/proc/cpuinfo`.
2. Detector hiện đại chạy **native C++/JNI** đọc thẳng `__system_property_get` + `/proc` → **né hook tầng Java**. ⟹ phải spoof ở **tầng system-property/build.prop thật**.
3. **TikTok riêng:** danh tính lõi = **`device_id`+`install_id`(+`cdid`) của ByteDance** (`log-va.tiktokv.com/service/2/device_register/`). **Giữ ổn định + làm ấm account** > pass Google Play Integrity.

---

## 1. Tín hiệu phát hiện (đã xác minh)
| Tín hiệu | Chi tiết | Mức |
| :-- | :-- | :--: |
| build.prop qemu | `ro.kernel.qemu=1`, `ro.hardware=goldfish/vbox86/nox`, `qemu.*` | Cao |
| FINGERPRINT/Build | `generic`, tag `test-keys`, MODEL/PRODUCT chứa `sdk` | Cao |
| File/socket ảo | `/dev/qemu_pipe`, `/dev/socket/qemud`, `/dev/socket/genyd`, `libc_malloc_debug_qemu.so` | Cao |
| **Native Bridge x86** | `ro.dalvik.vm.native.bridge`→lib dịch ARM, `hypervisor` cpuinfo, `vboxsf` mount | **Cao nhất, khó nhất** |
| Telephony mặc định | IMSI `310260000000000`, số `1555521555x`, operator `android` | T.bình |
| GPU renderer | SwiftShader/llvmpipe (ảo) vs Adreno/Mali (thật) | T.bình |
| Resolution/DPI ≠ model | màn hình không khớp thiết bị | T.bình |
| Hardware key attestation (TEE) | Play Integrity STRONG — prop spoof **không** vượt được | Cao |

## 2. Biện pháp đối phó & giới hạn trên MEmu
- **Sửa build.prop trực tiếp** (đổi `ro.product.*`/`ro.build.fingerprint`; xóa `ro.kernel.qemu`, `ro.hardware`, `qemu.*`) — đường chính.
- ⚠️ **MEmu = Android-x86 → nhiều module Magisk KHÔNG chạy** (MagiskHidePropsConf...). Đã ngừng phát triển.
- `kdrag0n/safetynet-fix` → chỉ SafetyNet cũ. `PlayIntegrityFix/Fork` → **chỉ spoof trong GMS/DroidGuard, KHÔNG giấu emulator khỏi TikTok**.
- Prop spoofing **không thể** vượt hardware key attestation (TEE) — giới hạn cứng.

## 3. Riêng TikTok
- Fingerprint lõi = ByteDance `device_id`/`install_id`/`cdid` (payload mã hóa `ttEncrypt`; host xoay theo vùng/version). Client tự sinh `openudid`(16 hex)/`ud_id`/`mc`.
- ✅ **Snapshot restore của dự án đã giữ đúng danh tính này** (backup /data/data → device_id không đổi).
- ❌ **BÁC BỎ:** "TikTok gate bằng Play Integrity/Universal SafetyNet Fix" (0-3). TikTok dùng SDK riêng.

## 4. Myth đã bác bỏ (đừng tốn công)
- IMEI toàn số 0 là dấu hiệu "chắc chắn" → **REFUTED** (nhiều máy thật cũng trả 0/null trên Android mới).
- Accelerometer trả giá trị cố định → **tranh cãi** (1-2).
- Play Integrity = thứ TikTok kiểm → **REFUTED**.
- "memuc không có lệnh spoof IMEI/model" → **REFUTED** (thực tế `setconfigex imei/microvirt_vm_model` CHẠY — đã kiểm chứng).

---

## 5. SCAN THỰC TẾ TRÊN MEMU CỦA DỰ ÁN (2026-07-02)

Máy ảo MEmu (bản mới nhất) **đã spoof tốt hơn kỳ vọng**:

| Kiểm tra | Kết quả | Đánh giá |
| :-- | :-- | :-- |
| `ro.kernel.qemu` | **rỗng** | ✅ sạch |
| `ro.hardware` | **`qcom`** | ✅ (giả Qualcomm) |
| `ro.bootmode` | `fastboot` | ✅ (không `unknown`) |
| `ro.build.fingerprint` | `Google/Google/Pixel 6:9/...:user/**release-keys**` | ✅ release-keys (không test-keys) |
| File QEMU/Geny/microvirt | **không có** | ✅ sạch |
| `/proc/mounts` vboxsf/9p | **không có** | ✅ sạch |
| goldfish tty driver | không có | ✅ sạch |
| **GPU renderer** | **`Qualcomm, Adreno (TM) 640`** | ✅ (không SwiftShader!) |
| **`ro.dalvik.vm.native.bridge`** | **`libnb.so`** (+ `persist.sys.nativebridge=1`) | ❌ **TELL** (x86 ARM-bridge) |
| **`/proc/cpuinfo` flags** | chứa **`hypervisor`** | ❌ **TELL** (CPU ảo hóa) |
| `ro.build.characteristics` | **`tablet`** | ⚠️ mismatch (Pixel 6 = phone) |
| `ro.product.device` | `Pixel 6` | ⚠️ nên là codename `oriole` |
| `/system/lib/libnb.so` | **CÓ** | ❌ TELL (lib dịch ARM) |

**Kết luận thực tế:** MEmu chỉ còn **3 điểm hở**:
- **Native Bridge** (`libnb.so` + 3 props) — **giới hạn cứng của x86**: gỡ đi thì app ARM (có thể gồm TikTok) không chạy. Chỉ ẩn được nếu dùng **image ARM** hoặc chấp nhận rủi ro.
- **`hypervisor` trong cpuinfo** — kernel-level, gần như không ẩn được nếu không vá kernel.
- **`ro.build.characteristics=tablet` + `ro.product.device`** — **SỬA ĐƯỢC** qua build.prop (cần root + remount).

## 6. Kế hoạch cho dự án (ưu tiên hiệu-quả-cao / công-sức-thấp)
| # | Việc | Trạng thái |
| :-- | :-- | :-- |
| 1 | Cột **Quốc gia · fingerprint · resolution khớp model** | ✅ đã làm |
| 2 | **Snapshot giữ device_id ByteDance** (backup /data/data) | ✅ đã làm |
| 3 | **Scan dấu vết ảo** (native check qua adb) — công cụ chẩn đoán | ⏳ đang thêm |
| 4 | **Harden props sửa được** (`ro.build.characteristics`, `ro.product.device`, xóa `qemu.sf.*`) khi chuẩn bị base | ⏳ đang thêm |
| 5 | Native Bridge + hypervisor flag | ⚠️ giới hạn x86 — ghi nhận, cân nhắc image ARM |

**Nguồn chính:** strazzere/anti-emulator · reveny/Android-Emulator-Detection · ASIACCS'14 · PlayIntegrityFork · kdrag0n/safetynet-fix · Loukious/TikTokDeviceGenerator · MEMUC manual.
