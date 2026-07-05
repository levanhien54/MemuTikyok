# Provision base image MEmu: Magisk + resetprop (khóa model)

> Phương án **(A)** từ [ANTI_DETECTION_UPGRADE.md](ANTI_DETECTION_UPGRADE.md). Mục tiêu:
> đưa **`resetprop`** (applet Magisk) vào base image MEmu để MPM **khóa `ro.product.model`
> + đồng bộ `ro.build.fingerprint`** mỗi lần khởi chạy — chống MEmu random model khi boot.
>
> ✅ **Trạng thái CODE (2026-07-06):** MPM đã sẵn sàng dùng resetprop:
> - `AdbWorker::lock_device_identity` gọi resetprop sau boot + **VERIFY** (đọc lại
>   `ro.product.model` = model đã khóa) → trả `true/false`. Gọi trong `provision` mỗi lần Chạy.
> - **Scan "Kiểm tra dấu vết ảo"** có mục **"Magisk/resetprop (khóa model)"**: đỏ = thiếu
>   resetprop (model bị ghi đè), sạch = base đã sẵn sàng. Đây là cách kiểm base image nhanh nhất.
> - **LogsView** hiện log `lock_device_identity` (warn nếu thiếu resetprop).
>
> Việc còn lại (một-lần, thủ công, TRÊN MÁY BẠN): đưa resetprop vào **ảnh hệ thống DÙNG CHUNG**
> của MEmu để mọi VM `run_profile` tạo mới đều thừa hưởng (xem mục "Base image" bên dưới).

## Vì sao cần resetprop (không dùng được cách khác)
- `ro.product.model` là prop **read-only** — sau boot `setprop` bị `property_service` chặn.
- Sửa `/system/build.prop` + reboot **không ăn** vì MEmu ghi đè model **mỗi lần boot**.
- Chỉ **`resetprop`** (ghi thẳng `prop_area`, bypass property_service) đổi được `ro.*`
  tại runtime. MPM chạy nó **sau `wait_boot_completed`** (muộn hơn cả lúc MEmu ghi đè) → thắng.

## Yêu cầu
- MEmu có root (MPM tự đặt `enable_su=1`).
- **Chỉ dùng Magisk CHÍNH THỐNG** (github.com/topjohnwu/Magisk) hoặc Magisk Delta.
  KHÔNG dùng bản repack lạ.

## Cách 1 — MagiskOnEmu (khuyến nghị, hỗ trợ MEmu trực tiếp)
1. Tải script chính thức: https://github.com/code871/MagiskOnEmu (GPL-3.0, 100% shell).
2. Trên **một VM MEmu đang chạy** (sẽ dùng làm base image), đẩy script và chạy theo README
   của repo (nó tự tải Magisk chính thức + cài vào system partition của emulator).
3. Khởi động lại VM. Xác minh:
   ```
   memuc -i <idx> adb "shell su -c 'command -v resetprop || ls /data/adb/magisk/resetprop'"
   memuc -i <idx> adb "shell su -c 'resetprop ro.product.model TEST && getprop ro.product.model'"
   ```
   → phải in ra đường dẫn resetprop và `TEST`.

## Cách 2 — Magisk Delta (cài vào system partition cho emulator)
- Dùng khi Cách 1 flaky với MEmu (có báo cáo). Xem README Magisk Delta cho emulator.

## Base image cho MÔ HÌNH DISPOSABLE (quan trọng — đã đổi kiến trúc)

⚠️ MPM **KHÔNG còn clone/pool**. `run_profile` tạo VM mới bằng `memuc create` mỗi lần Chạy.
Nên **KHÔNG** cài Magisk vào một VM riêng lẻ (VM đó bị hủy sau mỗi phiên) — phải đưa resetprop
vào **ẢNH HỆ THỐNG DÙNG CHUNG** của MEmu để mọi VM `create` mới đều thừa hưởng.

- MEmu chia sẻ **partition system ~1.3GB** giữa các VM; mỗi VM chỉ là delta mỏng (~50MB). Cài
  Magisk vào partition system dùng chung này (qua MagiskOnEmu/Magisk Delta ở trên) → **mọi VM
  tạo sau đều có resetprop**, không cần clone.
- Nếu bản MEmu của bạn KHÔNG chia sẻ system (mỗi VM một system riêng): cài vào **ảnh master/mẫu**
  mà `memuc create` sao chép từ đó (tùy phiên bản MEmu). Nếu cả hai bất khả → resetprop không
  phổ biến được cho luồng disposable; chấp nhận known-gap model (xem ANTI_DETECTION_UPGRADE.md).
- **Kiểm nhanh sau khi cài:** trong MPM, Chạy 1 profile → mở menu ⋮ → "Kiểm tra dấu vết ảo".
  Mục **"Magisk/resetprop (khóa model)"** phải **KHÔNG** đỏ (sạch). Nếu vẫn đỏ → VM `create`
  chưa thừa hưởng resetprop (system chưa được sửa đúng chỗ dùng chung).

## MPM tự khóa model — KHÔNG cần Magisk module
MPM gọi `lock_device_identity` **sau boot mỗi lần launch/provision/clone/swap**:
```
resetprop ro.product.model        <model>
resetprop ro.product.brand        <brand>
resetprop ro.product.manufacturer <manufacturer>
resetprop ro.product.device       <device>
resetprop ro.product.name         <device>
resetprop ro.build.fingerprint    <build_fingerprint>
```
→ Không cần dựng Magisk module boot-script; MPM re-assert mỗi lần chạy (đúng lúc, sau
MEmu override). (Tùy chọn: thêm module `post-fs-data`+`late_start service.d` nếu muốn
khóa cả trong lúc boot trước khi MPM gọi.)

## Xác minh cuối (sau khi cài Magisk vào system dùng chung)
1. Trong MPM: Chạy 1 profile → tab **Logs** xem `lock_device_identity` (không còn warn "VM chưa
   có resetprop"); menu ⋮ → **"Kiểm tra dấu vết ảo"** → mục "Magisk/resetprop (khóa model)" SẠCH.
2. `memuc -i <idx> adb "shell getprop ro.product.model"` = model của profile (không phải MEmu random).
3. `getprop ro.build.fingerprint` = fingerprint thật khớp model.
4. (App dò) cài "Emulator Detector"/"Momo" kiểm — LƯU Ý native-bridge + hypervisor **vẫn lộ**
   (cố hữu x86), model/fingerprint nay nhất quán.

## Giới hạn còn lại (không khắc phục ở tầng này)
- **native-bridge (libnb.so)** + **cờ hypervisor cpuinfo** vẫn lộ trên MEmu x86 —
  muốn hết phải chuyển ARM image/redroid-ARM. Xem [ANTI_DETECTION_UPGRADE.md](ANTI_DETECTION_UPGRADE.md).
