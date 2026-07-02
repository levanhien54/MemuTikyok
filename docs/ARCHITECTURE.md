# Kiến trúc MPM

Tài liệu này tóm tắt các quyết định kiến trúc quan trọng để **dễ quản lý, sửa lỗi và nâng cấp**. Chi tiết đầy đủ: §8 trong [`../kehoac.md`](../kehoac.md).

## Nguyên tắc cốt lõi
1. **Một biên giới tương tác OS duy nhất.** Mọi lệnh `memuc`/`adb` đi qua trait `MemucClient`. Không nơi nào khác trong code được spawn process MEmu. → dễ test, dễ thích ứng khi MEmu đổi format (R-07).
2. **Nguồn sự thật của trạng thái là polling**, không phải kết quả lệnh (§7.2). Lệnh `memuc` bất đồng bộ; UI hiển thị Pending tới khi poller xác nhận.
3. **UI không biết Tauri.** Frontend chỉ phụ thuộc interface `Backend` (`src/lib/backend.ts`). Có 2 hiện thực: `tauriBackend` (thật) và `mockBackend` (chạy độc lập/ test). Chọn tự động qua `isTauri()`.
4. **Kiểm soát tải bằng Command Queue.** Bulk action đi qua `Semaphore(K)` để không làm treo host (R-01).
5. **An toàn theo mặc định.** argv thay vì shell string (chống injection); validate input; Tauri capabilities tối thiểu.

## Luồng dữ liệu
```
UI (React) --invoke--> Tauri command --queue--> MemucClient --> memuc.exe
                                                     |
Poller (interval) ---------------------------------> |
   |                                                 v
   └── emit "instances:update" ---> Zustand store ---> UI cập nhật
```

## Ánh xạ FE ↔ BE
| UI (TS)                     | Backend (Rust)                         |
|-----------------------------|----------------------------------------|
| `src/lib/backend.ts` (interface) | `src-tauri/src/memuc/mod.rs` (trait) |
| `tauriBackend.ts` → `invoke` | `commands.rs` (`#[tauri::command]`)   |
| `types/instance.ts`         | `model.rs` (serde camelCase)           |
| event `instances:update`    | `poller.rs` → `app.emit(...)`          |

> Kiểu dữ liệu hiện khai báo tay hai phía. **Nâng cấp đề xuất:** sinh types tự động từ Rust bằng `ts-rs` để loại lệch schema.

## Điểm mở rộng (nâng cấp tương lai)
- **Module Automation:** thêm trait mới + entry trong sidebar; không đụng lõi memuc.
- **Giả lập khác (LDPlayer…):** thêm hiện thực `MemucClient` mới, chọn theo settings.
- **Persist settings:** `save_settings` hiện in-memory; nối vào file JSON ở app config dir.
- **Virtualization:** danh sách đã tách `InstanceRow` memo hóa; gắn `@tanstack/react-virtual` khi số VM lớn.

## Chiến lược test
- **Parser** (`memuc/parser.rs`): pure function, phủ nhiều fixtures (§7.3).
- **Queue** (`queue.rs`): kiểm chứng không vượt giới hạn song song.
- **Mock adapter**: test logic command không cần MEmu (`cargo test --features mock-memuc`).
- **Frontend**: Vitest cho helper/logic; (kế hoạch) Playwright + `tauri-driver` cho E2E.
