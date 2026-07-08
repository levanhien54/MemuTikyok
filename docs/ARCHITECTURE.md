# Kiến trúc MPM

Tài liệu này là nguồn mô tả kiến trúc hiện tại của MPM sau khi chuyển sang mô hình
**profile dùng một lần**.

## Trạng thái hiện tại

MPM quản lý **profile** chứ không quản lý VM bền vững.

- Profile là dữ liệu bền: tài khoản, ghi chú, quốc gia yêu cầu, fingerprint, snapshot session.
- VM là tài nguyên tạm: mỗi lần **Chạy** sẽ tạo VM sạch, áp fingerprint, cài TikTok nếu cần,
  restore snapshot mới nhất, mở app; mỗi lần **Dừng** sẽ backup rồi hủy VM.
- Giới hạn hiện tại: tối đa 5 profile chạy đồng thời.
- Đã bỏ mô hình cũ: warm pool, bulk instance action, proxy per-VM, poller nền phát
  event instance, API instance-centric.

## Nguyên tắc cốt lõi

1. **Tách profile khỏi VM.** Profile là nguồn dữ liệu bền; VM chỉ là môi trường chạy tạm.
2. **Biên OS rõ ràng.** Lệnh vòng đời/cấu hình VM đi qua `EmulatorClient`; thao tác adb đi qua
   `AdbWorker`. Không gọi shell nối chuỗi tùy tiện; mọi lệnh production dùng argv/command builder
   có timeout.
3. **Backup trước, hủy sau.** `teardown` chỉ remove VM sau khi backup, lưu blob snapshot, ghi DB
   thành công. Nếu backup lỗi, VM vẫn được giữ để retry.
4. **Fingerprint nhất quán theo profile.** Hardware profile sinh một lần khi tạo profile và được
   áp lại ở mọi lần chạy.
5. **Local-first.** SQLite và local snapshot store là implementation hiện tại; server/S3/lease là
   hướng mở rộng sau.
6. **Frontend không biết chi tiết Tauri/Rust.** UI chỉ dùng interface `Backend`; có Tauri backend
   thật và mock backend để chạy độc lập trong browser.

## Luồng Chạy/Dừng

```text
Create profile
  -> lưu AccountProfile + HardwareProfile vào SQLite

Run profile
  -> kiểm tra country gate nếu có
  -> reserve slot chạy
  -> EmulatorClient.create
  -> persist `running_vms` marker ngay khi xác định được vm_index
  -> apply hardware qua MuMu simulation
  -> EmulatorClient.start
  -> AdbWorker.wait_boot_completed
  -> push/resetprop nếu có Magisk APK
  -> re-assert runtime fingerprint (wm size/density + android_id + resetprop)
  -> debloat/harden best-effort
  -> install TikTok APK nếu cấu hình hoặc file mặc định tồn tại
  -> verify + restore snapshot mới nhất nếu có
  -> re-assert runtime fingerprint nếu vừa install/restore
  -> start TikTok
  -> ghi running profile và last_run_at

Stop profile
  -> mark profile đang teardown (vẫn chiếm slot)
  -> AdbWorker.backup
  -> SnapshotStore.put
  -> Db.record_snapshot trong transaction
  -> retention prune
  -> EmulatorClient.stop/remove
  -> clear running profile
```

## Thành phần chính

| Lớp | File hiện tại | Vai trò |
| --- | --- | --- |
| Frontend contract | `src/lib/backend.ts` | Interface duy nhất UI dùng |
| Tauri adapter | `src/lib/tauriBackend.ts` | Map `Backend` sang `invoke`/event |
| Mock adapter | `src/lib/mockBackend.ts` | Dev/test UI không cần Tauri |
| IPC commands | `src-tauri/src/commands.rs` | Adapter mỏng sang business logic |
| Profile lifecycle | `src-tauri/src/profile_ops.rs` | Create/list/update/delete/run/stop profile |
| Orchestrator | `src-tauri/src/orchestrator.rs` | Provision/teardown disposable VM |
| Emulator trait | `src-tauri/src/emulator/mod.rs` | Lifecycle/config VM |
| MuMu adapter | `src-tauri/src/emulator/mumu.rs` | Gọi `MuMuManager.exe` |
| ADB trait | `src-tauri/src/adb.rs` | Backup/restore/app install/harden/scan |
| SQLite | `src-tauri/src/db.rs` | Profile, snapshot, running VM reconcile |
| Snapshot store | `src-tauri/src/snapshot.rs` | Nén, mã hóa, verify, retention blob |
| App state | `src-tauri/src/state.rs` | Shared state, locks, in-memory running map |

## IPC hiện tại

Profile lifecycle:

- `create_profile`
- `list_profiles`
- `update_profile`
- `delete_profile`
- `run_profile`
- `stop_profile`

Tiện ích trên VM đang chạy:

- `scan_emulator`
- `run_watch_session`
- `upload_video_to_vm`

Cài đặt/chẩn đoán:

- `get_settings`
- `save_settings`
- `get_logs`

## Mapping FE/BE

| TypeScript | Rust |
| --- | --- |
| `src/types/instance.ts` | `src-tauri/src/model.rs` |
| camelCase JSON | `#[serde(rename_all = "camelCase")]` |
| `ProfileView.runningVm` | `ProfileView.running_vm` |
| `AppSettings.mumuPath` | `AppSettings.mumu_path` |

Các type vẫn khai báo tay hai phía. Hướng nâng cấp hợp lý là sinh type TS từ Rust bằng `ts-rs`
hoặc schema generator tương đương để tránh lệch schema.

## Runtime settings

- `pollIntervalMs` áp dụng ngay trong frontend: `useProfileStore` khởi tạo lại interval khi settings đổi.
- `maxConcurrency` áp dụng ngay trong backend: `save_settings` cập nhật `CommandQueue`; nếu giảm thấp hơn số tác vụ đang chạy thì tác vụ hiện tại chạy xong, tác vụ mới chờ theo limit mới.
- `tiktokApkPath` được đọc ở mỗi lần `run_profile`, nên có hiệu lực ở lần chạy profile kế tiếp.
- `magiskApkPath` được trích lại và set vào `AppState` ngay khi lưu settings. Nếu để trống,
  backend fallback sang `Magisk-v30.7.apk` đi kèm Tauri resource (hoặc bản dev trong
  `src-tauri/resources`) nếu có.
- `mumuPath` hiện chỉ dùng lúc khởi động để dựng `EmulatorClient`/`AdbWorker`; đổi đường dẫn được persist nhưng cần mở lại app để adapter production chuyển sang binary mới. Muốn áp dụng nóng cần refactor state sang adapter có thể reload.

## MuMu/ADB

- `EmulatorClient` dùng `MuMuManager.exe info -v all`, `clone`, `control launch/shutdown`,
  `delete`, và `simulation`.
- Fingerprint production dùng `MuMuManager.exe simulation -v <idx> -sk <key> -sv <value>`:
  `microvirt_vm_model`, `microvirt_vm_brand`, `microvirt_vm_manufacturer`,
  `mac_address`, `enable_su`, `custom_resolution`; `imei` chỉ set khi profile có TAC đã verify
  (không bịa IMEI random khi TAC rỗng).
- Runtime fingerprint sau boot dùng ADB: `wm size/density`, `settings put secure android_id`,
  và `resetprop` cho `ro.product.*`/`ro.build.fingerprint` nếu có Magisk APK cấu hình hoặc bundled.
- `android_id` áp qua adb, không coi là khóa MuMuManager đáng tin cậy; Android 8+ SSAID/GMS
  vẫn có thể cấp giá trị app-scoped khác sau khi TikTok chạy.
- `AdbWorker` production dùng `MuMuManager.exe adb -v <idx> -c "<adb command>"`.

## Đồng bộ trạng thái

Không còn poller nền phát event instance. UI tự gọi `list_profiles` khi:

- app khởi tạo,
- user refresh,
- focus cửa sổ,
- polling nhẹ trong store frontend.

`profile_ops::list` chỉ gọi emulator khi có profile đang chạy để reconcile VM đã biến mất ngoài
luồng app.

Khi app khởi động, `reconcile_startup` đọc bảng `running_vms`; VM nào còn sống từ phiên crash
trước sẽ bị stop/remove. Row chỉ được xóa sau khi remove thành công; nếu remove lỗi, row được giữ
để lần khởi động sau retry thay vì mất dấu VM mồ côi.

## Chiến lược test

- Frontend: `npm run typecheck`, `npm run lint`, `npm run test`, `npm run build`.
- Rust unit/integration nhẹ: `cargo test`.
- Rust lint nghiêm: `cargo clippy --all-targets --all-features -- -D warnings`.
- Mock emulator build: `cargo test --features mock-emulator`.
- E2E thật MuMu: `cargo test --lib e2e_real -- --ignored --nocapture`.

## Những giới hạn đã biết

- MuMu x86 vẫn lộ native bridge/hypervisor; docs anti-detection coi đây là giới hạn nền tảng.
- `android_id` có thể bị GMS/MuMu ghi đè sau khi cài/chạy TikTok; MPM re-apply trước start app
  nhưng chưa khóa cứng SSAID mà TikTok thật sự nhìn thấy.
- Country gate hiện kiểm IP thoát của host, không phải geo tách riêng từng VM.
- `mumuPath` đổi trong Settings chưa reload adapter MuMu/ADB trong tiến trình đang chạy; mở lại app để dùng đường dẫn mới.
- Server repository, cloud storage, proxy sticky per-account, lease/heartbeat vẫn là hướng mở rộng,
  chưa phải code hiện tại.
