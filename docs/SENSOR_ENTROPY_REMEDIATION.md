# Sensor entropy gap: nghiên cứu và hướng xử lý

## Kết luận ngắn

`spoof sensor entropy` không phải bài toán `setprop`. TikTok/app native đọc cảm biến qua Android
Sensor Framework (`SensorManager`/native sensor APIs). Vì vậy muốn xử lý chắc phải có **event stream**
ở tầng framework/HAL, không chỉ khai báo tên thiết bị hay sửa `build.prop`.

Hiện tại MPM nên xử lý theo 3 tầng:

1. **Đo baseline** trên MuMu thật: thiếu cảm biến nào, provider có lộ chuỗi giả lập không.
2. **Tối ưu ít rủi ro**: giữ profile ổn định, giảm các tell đã sửa được, không thêm Frida/LSPosed
   chỉ để fake sensor nếu chưa đo được lợi ích.
3. **Chỉ khi cần mạnh hơn**: làm base image có virtual Sensor HAL/bridge event stream, hoặc hook
   process bằng Zygisk/LSPosed với rủi ro phát hiện ngược rõ ràng.

Nguồn nền:

- Android sensor overview: <https://developer.android.com/develop/sensors-and-location/sensors/sensors_overview>
- Android Sensor HAL/AOSP: <https://source.android.com/docs/core/interaction/sensors>
- Android Emulator console có lệnh `sensor set`, nhưng đây là emulator chính chủ, không phải API
  MuMuManager: <https://developer.android.com/studio/run/emulator-console>

## Tầng 0 — Diagnostic đã thêm

Chạy:

```powershell
cd D:\MemuTiktok\src-tauri
cargo test --lib e2e_real::a16_sensor_entropy_baseline -- --ignored --nocapture
```

A.16 sẽ:

- Provision một VM MuMu thật.
- Đọc `pm list features | grep -i sensor`.
- Đọc `dumpsys sensorservice`.
- Kiểm accelerometer, gyroscope, magnetometer/compass.
- Cảnh báo provider có chuỗi `goldfish`, `ranchu`, `qemu`, `virtual sensor`, `mock sensor`.

`scan_emulator_tells` cũng đã có 2 mục mới:

- `Motion sensors`
- `Sensor provider tells`

## Tầng 1 — Tối ưu ít rủi ro trong MPM hiện tại

Giữ nguyên nguyên tắc: không thêm hook layer khi chưa cần.

- Không dùng Frida để fake sensor: port/gadget/ptrace là bề mặt bị dò mạnh.
- Không dùng AndroidFaker/LSPosed chỉ vì sensor nếu chưa có bằng chứng từ A.16/B2.6; LSPosed/Zygisk
  tự nó là tell mới.
- Ưu tiên làm sạch các lớp đã có mitigation: model/fingerprint, resolution runtime, root-hide
  nếu base image có Magisk/Shamiko, snapshot giữ `device_id/install_id`.

Nếu A.16 PASS đủ sensor nhưng vẫn nghi entropy thấp, cần test app nhỏ hoặc log native để đo event
rate/jitter. `dumpsys sensorservice` chỉ xác nhận bề mặt, chưa chứng minh stream giống máy thật.

## Tầng 2 — Virtual Sensor HAL / host-to-guest bridge

Đây là hướng sạch nhất nếu muốn làm bài bản:

- Base image cung cấp sensor HAL có accelerometer/gyro/magnetometer/rotation-vector.
- Mỗi profile có `sensor_seed` ổn định để sinh bias/noise riêng.
- Event stream phải có tương quan vật lý:
  - accelerometer luôn có gravity gần 9.81 m/s², có drift nhỏ;
  - gyroscope dao động nhỏ quanh 0 khi không xoay, spike khi có thao tác;
  - magnetometer có field nền ổn định theo vùng, nhiễu nhẹ;
  - rotation vector nhất quán với accel/gyro/magnetometer.
- Sampling rate phải hợp lý, không phải hằng số hoặc nhiễu trắng độc lập hoàn toàn.

Nhược điểm: cần sửa image/HAL, khó làm trên MuMu đóng. Hợp hơn với Redroid/custom image hoặc
hạ tầng Android tự kiểm soát.

## Tầng 3 — Hook theo process (Zygisk/LSPosed)

Chỉ cân nhắc nếu A.16 fail và dữ liệu ban/risk chứng minh sensor là nguyên nhân đáng kể.

Ưu điểm:

- Có thể fake riêng cho TikTok process.
- Không cần sửa HAL toàn hệ thống.

Rủi ro:

- Zygisk/LSPosed/module list/native traces có thể bị phát hiện ngược.
- Java-only hook không đủ nếu app đọc native sensor APIs.
- Cần giữ consistency với rotation/vector/gravity, nếu không entropy giả lại thành tell mới.

Kết luận: đây là hướng **rủi ro trung/cao**, không nên gộp vào MPM mặc định.

## Quyết định đề xuất cho MPM

1. Chạy A.16 trên MuMu thật và ghi kết quả vào E2E runbook.
2. Nếu thiếu sensor cơ bản: ghi là platform gap; không claim đã spoof.
3. Nếu đủ sensor nhưng entropy kém: thêm một diagnostic app/test sau để đo event rate/jitter.
4. Chỉ đầu tư Sensor HAL/bridge khi quy mô đủ lớn hoặc khi chuyển sang Redroid/custom image.
5. Tránh Frida/AndroidFaker mặc định vì tăng bề mặt bị dò nhiều hơn lợi ích dự kiến.
