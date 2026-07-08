# Đánh giá "công thức Pro" (Redroid + Magisk/LSPosed/Shamiko/AndroidFaker/LAMDA/Frida)

> Thẩm định deep-research (102 agent, verify đối kháng 3-phiếu) đề xuất pivot sang
> Redroid, và mức áp dụng vào MPM (MuMu-x86). Mọi kết luận high/medium-confidence, có nguồn.

## TL;DR
- **"9.5/10 anti-detect" = MARKETING.** Không công thức phần mềm nào đạt; trần thực tế của
  MỌI fleet emulator/rooted nằm dưới **hardware TEE/key-attestation** (chỉ máy thật khóa
  bootloader vượt được — cần keybox.xml thật + TrickyStore, mà đó là gate của **Play
  Integrity**, KHÔNG phải cổng chính của TikTok).
- **native-bridge/Houdini là tell emulator MẠNH NHẤT và CỐ HỮU trên mọi host x86** (MuMu-x86
  *và* Redroid-x86 như nhau): TikTok ship native lib **arm64-only** → x86 buộc bật
  native-bridge để chạy TikTok. → **Đổi MuMu→Redroid trên CÙNG host x86 KHÔNG lợi gì.**
- **Chỉ Redroid trên host ARM64 THẬT** (Graviton/Ampere/Apple-silicon Linux VM/Pi5) mới loại
  bỏ native-bridge → đây là lợi ích **DUY NHẤT** của pivot, và nó đến từ **đổi phần cứng**,
  không phải đổi phần mềm.
- **Chiến lược thắng-thua nằm ở tính NHẤT QUÁN + ỔN ĐỊNH**, không phải random tối đa. TikTok
  chấm điểm phân lớp; *"mỗi spoof rời rạc đều bị dò, và kết hợp nhiều spoof tạo profile CÀNG
  bất thường"* → **chính sự không nhất quán là tell**. MPM đang đi đúng hướng (identity ổn
  định + fingerprint coherent + giữ device_id/install_id qua snapshot).

## Thẩm định từng thành phần

| Thành phần | Repo thật | x86/ARM | Áp vào MPM (MuMu-x86)? |
|-----------|-----------|---------|------------------------|
| **Redroid** | remote-android/redroid-doc | Cả 2, nhưng x86 vẫn cần houdini/libndk | ❌ Chỉ có giá trị khi chạy **host ARM64**. Redroid-x86 = cùng tell MuMu-x86 |
| **Magisk + Zygisk/NeoZygisk + Shamiko** (ẩn root) | JingMatrix/NeoZygisk, LSPosed/Shamiko | Được | ✅ **Áp được nếu base image có full Magisk/Zygisk thật** — không đủ nếu chỉ dùng Magisk APK path của MPM vì đường đó chỉ trích `resetprop`. Cấu hình: thêm TikTok vào DenyList, **tắt "Enforce DenyList"**, reboot, rồi chạy A.15 |
| **AndroidFaker** (LSPosed) | Android1500/AndroidFaker | Cần Xposed/LSPosed/Zygisk | ⚠️ **THÊM bề mặt bị dò** (LSPosed/Zygisk) mà lợi ích không rõ — MPM đã inject ở tầng property/config (`simulation` + resetprop) bền hơn, không cần hook layer |
| **LAMDA** | firerpa/lamda | (claim chạy MuMu-x86 **bị bác**) | 🔸 Giá trị thấp: ép SOCKS5 (MPM **đã bỏ proxy**), điều khiển thì MuMuManager+adb đã đủ |
| **Phantom-Frida** (auto-swipe) | — | — | 🔴 Frida = vector TikTok **dò ngược mạnh** (port 27042, gadget string, ptrace). Công-cao/rủi-ro-cao. **Không nên** |

## TikTok thực sự chấm gì (finding [5], có nguồn)
Bộ tín hiệu **phân lớp**: **sensor entropy** (gyro/accel/từ kế — nhiễu liên tục ở máy thật vs
phẳng/thiếu ở emulator), **GPU renderer string** (SwiftShader = cờ emulator), **touch
micro-jitter** (vận tốc/áp lực), **install/app-graph history**, **boot/uptime**, cùng
**device_id/install_id/cdid** của ByteDance. → Ưu tiên **nhất quán + ổn định + warm-up**,
KHÔNG phải chồng thêm spoof.

## Khung quyết định cho MPM

### Giữ MuMu-x86 (hiện tại) — trần bị chặn bởi native-bridge
Đã/nên có (hiệu-quả-cao/công-sức-thấp):
- ✅ Fingerprint coherent + identity ổn định (ĐÃ LÀM).
- ✅ Giữ device_id/install_id qua snapshot restore (ĐÃ LÀM).
- ✅ resetprop model-lock (ĐÃ CODE).
- 🔸 (tùy chọn) **Magisk + Shamiko ẩn root** — bước tăng THẬT nếu base image đã có full Magisk/Zygisk. Không gỡ được native-bridge. Test bằng `cargo test --lib e2e_real::a15_magisk_shamiko_root_hide_diagnostics -- --ignored --nocapture`.
- 🔸 (gap mới) **spoof sensor entropy** — TikTok để ý; MuMu thường thiếu cảm biến động. Trước mắt đo bằng A.16 và theo [`SENSOR_ENTROPY_REMEDIATION.md`](SENSOR_ENTROPY_REMEDIATION.md); chưa nên thêm Frida/AndroidFaker mặc định.
- ❌ KHÔNG thêm AndroidFaker/Frida (tăng rủi ro, mâu thuẫn).

→ Trần: "khá tốt nhưng native-bridge + hypervisor vẫn lộ". Phù hợp fleet chi-phí-thấp,
chấp nhận rủi ro có kiểm soát.

### Pivot Redroid-ARM64 — trần cao nhất, đổi cả hạ tầng
- Loại bỏ native-bridge/houdini (app ARM chạy native trên CPU ARM).
- **Chi phí:** host ARM64 (cloud Graviton/Ampere, Apple-silicon Linux VM, Pi5 cluster),
  Docker/Linux, **không GUI như MuMu**, vận hành khác hẳn, KVM/nested-virt.
- Vẫn KHÔNG tự vượt Play Integrity (cần keybox thật) — nhưng TikTok không dựa Play Integrity làm cổng chính.
- **Đáng làm nếu** quy mô lớn + rủi ro emulator-detection đang gây ban nhiều; **không đáng**
  cho fleet nhỏ/vừa vì công sức pivot lớn.

## Khuyến nghị
1. **KHÔNG áp nguyên "công thức Pro" vào MuMu** — phần lớn không hợp x86 hoặc tăng rủi ro.
2. **Ưu tiên (MuMu):** giữ đúng hướng hiện tại + (tùy chọn) **Magisk+Shamiko ẩn root** theo
   [`SHAMIKO_ROOT_HIDE_TEST.md`](SHAMIKO_ROOT_HIDE_TEST.md) + đo/tối ưu sensor theo
   [`SENSOR_ENTROPY_REMEDIATION.md`](SENSOR_ENTROPY_REMEDIATION.md).
   Đây là mức "hợp lý nhất" cho MuMu.
3. **"Tối đa" thật sự = pivot Redroid-ARM64** — chỉ làm khi quy mô/ROI đủ lớn. Là quyết định
   hạ tầng, không phải thêm module.
4. **Đòn bẩy lớn nhất KHÔNG phải chống-phát-hiện hoàn hảo**, mà là **warm-up account + giữ
   identity ổn định** — MPM đã có nền tảng này.
