# Khóa model qua Magisk resetprop (standalone — KHÔNG cần base image)

> Phương án **(A)** từ [ANTI_DETECTION_UPGRADE.md](ANTI_DETECTION_UPGRADE.md). Mục tiêu:
> **khóa `ro.product.model` + đồng bộ `ro.build.fingerprint`** mỗi lần Chạy — chống MuMu
> random model khi boot.
>
> ✅ **Đã hiện thực (2026-07-06):** MPM tự làm HẾT, chỉ cần bạn trỏ **Magisk APK** trong
> Cài đặt. KHÔNG cần MagiskOnEmu / Magisk Delta / sửa base image thủ công nữa.

## Cách hoạt động (đã kiểm chứng trên MuMu thật)

MuMu có **root native** (`enable_su=1` → `uid=0`) nhưng **không** có Magisk/resetprop.
`resetprop` là applet của binary `magisk`, đóng gói sẵn trong **Magisk APK** dưới
`lib/<abi>/libmagisk.so`. Vì đã có root, MPM chỉ cần **đẩy binary vào VM và chạy
`magisk resetprop`** — không cần cài Magisk vào hệ thống. Hợp mô hình disposable (VM tạo
mới mỗi lần Chạy): binary được đẩy lại mỗi lần provision.

Luồng tự động trong `provision` (sau `wait_boot_completed`):
1. **Khởi động (một lần):** `magisk::ensure_binary` trích `lib/x86_64/libmagisk.so` từ APK →
   cache `%APPDATA%\com.mpm.manager\magisk\magisk-x86_64`. MuMu = x86_64 (đã kiểm chứng).
2. **Mỗi provision:** `push_resetprop` đẩy binary vào `/data/local/tmp/magisk` + `chmod 755`
   + verify `magisk -c` (ra version, vd `30.7:MAGISK:R (30700)`).
3. `lock_device_identity` **sinh script** đặt bộ prop nhận diện thiết bị rồi chạy `sh <file>` (xem "Vì sao script").
4. **VERIFY:** đọc lại `ro.product.model` = model đã khóa → trả `true/false` (log ở LogsView).

## Cấu hình (việc DUY NHẤT bạn cần làm)

1. Tải **Magisk APK chính thống** ([github.com/topjohnwu/Magisk/releases](https://github.com/topjohnwu/Magisk/releases)
   — đã kiểm chứng v30.7). KHÔNG dùng bản repack lạ.
2. Trong MPM → **Cài đặt** → **"Magisk APK (khóa model)"** → trỏ tới file `.apk`
   (vd `D:\MemuTiktok\appTiktok\Magisk-v30.7.apk`).
3. Xong. Mỗi lần Chạy profile, MPM tự đẩy resetprop + khóa model.
   - Để trống ô này = **tắt** khóa model (model sẽ bị MuMu ghi đè).

## Vì sao cần resetprop (không dùng được cách khác)
- `ro.product.model` là prop **read-only** — sau boot `setprop` bị `property_service` chặn.
- Sửa `/system/build.prop` + reboot **không ăn** vì MuMu ghi đè model **mỗi lần boot**.
- Chỉ **`resetprop`** (ghi thẳng `prop_area`, bypass property_service) đổi được `ro.*`
  tại runtime. MPM chạy nó **sau `wait_boot_completed`** (muộn hơn lúc MuMu ghi đè) → thắng.

## Vì sao sinh SCRIPT thay vì chạy từng lệnh resetprop
Value model thường **có khoảng trắng** ("Redmi Note 8", "Pixel 7"). Nếu chạy
`adb shell su -c 'magisk resetprop ro.product.model "Redmi Note 8"'`, chuỗi bị **3 tầng sh**
(MuMuManager → adbd → `su -c`) tách lại → resetprop nhận 3 tham số, **model KHÔNG khóa** (kiểm
chứng thực: brand/device/fingerprint đổi được, model thì không). Cách chắc chắn: MPM **sinh
file script**, đẩy vào VM, chạy `sh <file>` — sh đọc nháy kép **từ file** nên value giữ
nguyên. (`resetprop -f <propfile>` bị SELinux chặn đọc file → không dùng được.)

Các prop được khóa (nhất quán fingerprint):
```
ro.product.model        <model>
ro.product.brand        <brand>
ro.product.manufacturer <manufacturer>
ro.product.device       <device>
ro.product.name         <device>
ro.build.fingerprint    <build_fingerprint>
```

## Xác minh
1. **Scan "Kiểm tra dấu vết ảo"** (menu ⋮ trên VM đang chạy) → mục
   **"Magisk/resetprop (khóa model)"** phải **KHÔNG đỏ** (sạch = có resetprop). Đây là cách
   kiểm nhanh nhất. Đỏ = chưa trỏ Magisk APK, hoặc APK hỏng/không có `lib/x86_64/libmagisk.so`.
2. `MuMuManager.exe adb -v <idx> -c "shell getprop ro.product.model"` = model của profile (không phải MuMu random).
3. `getprop ro.build.fingerprint` = fingerprint thật khớp model.
4. **Test tự động:** `cargo test --lib a12_khoa_model_co_khoang_trang -- --ignored --nocapture`
   provision VM thật, khóa model **có khoảng trắng** ("Redmi Note 8"), verify runtime, rồi hủy VM.

## Giới hạn còn lại (không khắc phục ở tầng này)
- **native-bridge (libnb.so)** + **cờ hypervisor cpuinfo** vẫn lộ trên MuMu x86 —
  muốn hết phải chuyển ARM image/redroid-ARM. Xem [ANTI_DETECTION_UPGRADE.md](ANTI_DETECTION_UPGRADE.md).
