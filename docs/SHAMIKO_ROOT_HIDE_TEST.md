# Magisk + Shamiko root-hide: kế hoạch xử lý và test

## Kết luận ngắn

MPM hiện có **Magisk APK path** trong Settings, nhưng đường này chỉ dùng để trích
`libmagisk.so` làm binary `magisk resetprop` standalone. Nó **không cài full Magisk**,
không có `magiskd`, không có Zygisk, và vì vậy **không thể bật Shamiko** bằng cấu hình này.

Muốn test/áp dụng Shamiko thật cần base image MuMu đã có:

1. Full Magisk hoạt động trong Android image.
2. Zygisk bật.
3. Shamiko module đã cài và reboot.
4. TikTok nằm trong Magisk DenyList.
5. **Enforce DenyList tắt**. Shamiko dùng danh sách DenyList làm hidelist; nếu Enforce bật,
   Shamiko không xử lý đúng mô hình ẩn root.

Nguồn đối chiếu:

- Magisk tools/resetprop/DenyList: <https://topjohnwu.github.io/Magisk/tools.html>
- Magisk project: <https://github.com/topjohnwu/Magisk>
- Shamiko releases của LSPosed: <https://github.com/LSPosed/LSPosed.github.io/releases>

## Vì sao không dùng bind-mount `su`

Trước đây script `lock_device_identity` từng bind mount `/system/bin/ls` đè lên
`/system/bin/su` và `/system/xbin/su`. Cách đó không tương đương Shamiko:

- Nó là global namespace, không phải ẩn theo process/app như Shamiko.
- Có thể làm hỏng các bước MPM cần root sau đó (`harden`, re-assert fingerprint, media upload).
- Nó không che Magisk/Zygisk/module/process/mount traces mà app native có thể soi.

Vì vậy MPM không tự ẩn root bằng bind-mount `su` nữa. Root-hide đúng hướng là cấu hình full
Magisk/Zygisk/Shamiko trong base image rồi test bằng A.15.

## Test tự động đã thêm

Chạy:

```powershell
cd D:\MemuTiktok\src-tauri
cargo test --lib e2e_real::a15_magisk_shamiko_root_hide_diagnostics -- --ignored --nocapture
```

Test này sẽ:

- Tạo/provision VM MuMu thật.
- Cài TikTok APK mặc định.
- Kiểm `magisk -c`.
- Kiểm `magisk --denylist status`.
- Kiểm `magisk --denylist ls` có `com.zhiliaoapp.musically`.
- Kiểm dấu vết module/status Shamiko dưới `/data/adb`.
- In báo cáo `A15_SHAMIKO_DIAG` để chép vào log test.

PASS khi:

- Có full Magisk trong image, không chỉ resetprop standalone.
- Có Shamiko module/status.
- DenyList chứa TikTok.
- Enforce DenyList đang tắt.

FAIL thường gặp:

| Lỗi | Ý nghĩa | Cách xử lý |
| :-- | :-- | :-- |
| `MISSING_MAGISK` | Base image chưa có full Magisk daemon | Cài Magisk vào image MuMu/base VM, reboot, clone lại |
| Không thấy `shamiko` | Chưa cài Shamiko module hoặc chưa reboot | Cài Shamiko đúng bản cho Zygisk, reboot |
| DenyList không có TikTok | TikTok chưa được thêm vào hidelist | Thêm `com.zhiliaoapp.musically` vào DenyList |
| Enforce DenyList chưa tắt | Shamiko không dùng được DenyList theo cách mong muốn | Tắt Enforce DenyList, reboot |

## Giới hạn thực tế

Shamiko chỉ giảm bề mặt **root detection**. Nó không xử lý được:

- Native bridge/Houdini trên MuMu x86.
- CPU hypervisor flag.
- Sensor entropy thiếu/không giống máy thật.
- Android 8+ SSAID/app-scoped `android_id` bị GMS/app cấp lại.

Vì vậy Shamiko là bước tăng điểm thực tế, nhưng không biến MuMu x86 thành thiết bị thật.
