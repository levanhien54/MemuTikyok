# Provision base image MEmu: Magisk + resetprop (khóa model)

> Phương án **(A)** từ [ANTI_DETECTION_UPGRADE.md](ANTI_DETECTION_UPGRADE.md). Mục tiêu:
> đưa **`resetprop`** (applet Magisk) vào base image MEmu để MPM **khóa `ro.product.model`
> + đồng bộ `ro.build.fingerprint`** mỗi lần khởi chạy — chống MEmu random model khi boot.
>
> ⚠️ **Trạng thái:** phần CODE của MPM đã sẵn sàng (`AdbWorker::lock_device_identity`
> gọi resetprop sau boot, no-op nếu chưa có resetprop). Bước provision dưới đây **chưa
> verify được trên máy hiện tại** vì MEmu đang không boot xong VM (cần restart MEmu/máy).

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

## Biến VM đã cài thành BASE IMAGE
- Sau khi resetprop chạy được trên 1 VM: dùng VM đó làm **base** để clone (MPM
  `warm_pool` / `clone_from_base` sẽ nhân bản — mọi clone thừa hưởng resetprop).
- Đặt `pool_base_index` = index VM base trong Settings.

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

## Xác minh cuối (sau khi provision)
1. Trong MPM, chạy 1 tài khoản → xem log/toast `lock_device_identity` trả `true`.
2. `memuc -i <idx> adb "shell getprop ro.product.model"` = model của account (không phải MEmu random).
3. `getprop ro.build.fingerprint` = fingerprint thật khớp model.
4. (App dò) cài "Emulator Detector"/"Momo" kiểm — LƯU Ý native-bridge + hypervisor **vẫn lộ**
   (cố hữu x86), model/fingerprint nay nhất quán.

## Giới hạn còn lại (không khắc phục ở tầng này)
- **native-bridge (libnb.so)** + **cờ hypervisor cpuinfo** vẫn lộ trên MEmu x86 —
  muốn hết phải chuyển ARM image/redroid-ARM. Xem [ANTI_DETECTION_UPGRADE.md](ANTI_DETECTION_UPGRADE.md).
