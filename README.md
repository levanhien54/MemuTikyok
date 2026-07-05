# MEmu Play Manager (MPM)

Ứng dụng desktop quản lý fleet máy ảo **MEmu Play** với giao diện hiện đại, hiệu năng cao và kiến trúc mở rộng được. Xây dựng bằng **Tauri (Rust) + React + TypeScript + TailwindCSS**.

> Đặc tả & kế hoạch đầy đủ: xem [`kehoac.md`](./kehoac.md) (SRS v3.0). Kiến trúc: [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md).

## Tính năng (v1.0)
- Giám sát trạng thái VM theo thời gian thực (polling, nguồn sự thật là `memuc listvms`).
- Vòng đời VM: start / stop / reboot / tạo / xóa / clone / đổi tên.
- Thao tác hàng loạt có **kiểm soát tải** (Command Queue giới hạn song song).
- Cấu hình CPU/RAM/độ phân giải/model.
- Tìm kiếm, lọc; Dark/Light mode; log viewer.

## Kiến trúc (tóm tắt)
- **Frontend** (`src/`): React + TS + Tailwind. UI **không** gọi thẳng `memuc`; mọi tương tác qua interface `Backend` (`src/lib/`) — có adapter Tauri thật và adapter **mock** để chạy độc lập trong trình duyệt.
- **Backend** (`src-tauri/src/`): Rust + Tauri. Toàn bộ tương tác OS đi qua trait `MemucClient` (`memuc/`) — có `RealMemuc` (gọi `memuc.exe`, argv an toàn) và `MockMemuc` (test). Kèm Command Queue (`queue.rs`), Poller (`poller.rs`), State registry (`state.rs`).

## Yêu cầu môi trường
| Công cụ | Ghi chú |
|--------|--------|
| Node ≥ 20 | frontend |
| Rust (stable) + MSVC build tools | backend Tauri — cài qua [rustup](https://rustup.rs) |
| WebView2 Runtime | có sẵn Win11; bundle cho Win10 |
| MEmu Play (mới nhất) | runtime; nếu chưa cài, app tự fallback mock |

## Chạy dự án

```bash
npm install

# 1) Chỉ frontend (không cần Rust) — dùng mock backend, xem UI ngay:
npm run dev            # http://localhost:1420

# 2) App desktop đầy đủ (cần Rust + Tauri):
npm run tauri:dev

# Kiểm thử & chất lượng
npm run test           # unit test frontend (Vitest)
npm run lint
npm run typecheck
cd src-tauri && cargo test --features mock-memuc   # test backend không cần MEmu
```

## Build cài đặt
```bash
npm run tauri:build    # sinh .msi / .exe trong src-tauri/target/release/bundle
```

## Cấu trúc thư mục
```
src/                    Frontend React
  components/           UI dùng chung (Button, StatusBadge, dialog…)
  features/             Màn hình: dashboard, instances, settings, logs
  lib/                  Backend abstraction (tauri + mock) + helpers
  store/                State (Zustand)
  types/                Kiểu chia sẻ FE↔BE
src-tauri/src/          Backend Rust
  memuc/                Trait MemucClient + parser (test) + real + mock
  queue.rs poller.rs state.rs commands.rs error.rs model.rs
docs/                   Tài liệu kiến trúc & đóng góp
```

## Trạng thái
Giai đoạn 0 (nền tảng & scaffold) — xem lộ trình §14 trong `kehoac.md`.
