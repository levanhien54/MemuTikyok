# MuMu Player 12 Manager (MPM)

## Agent workflow

This repository is set up to use
[obra/superpowers](https://github.com/obra/superpowers) as the preferred coding
agent workflow layer. Install the `Superpowers` plugin in Codex via `/plugins`;
the root `AGENTS.md` contains the project-level instructions, and
`docs/SUPERPOWERS_INTEGRATION.md` has the integration notes.

Ứng dụng desktop quản lý profile TikTok trên **MuMu Player 12** theo mô hình
**môi trường dùng một lần**: profile là dữ liệu bền, VM chỉ được tạo tạm khi
chạy và bị hủy sau khi backup snapshot. Xây dựng bằng **Tauri (Rust) + React +
TypeScript + TailwindCSS**.

> Đặc tả & kế hoạch đầy đủ: xem [`kehoac.md`](./kehoac.md) (SRS v3.0). Kiến trúc: [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md).

## Tính năng hiện tại
- Quản lý profile TikTok: tài khoản, ghi chú, quốc gia yêu cầu, fingerprint cố định.
- Chạy profile bằng VM disposable: clone VM sạch, áp fingerprint, restore snapshot,
  mở TikTok.
- Dừng profile an toàn: backup dữ liệu app, lưu snapshot local, ghi SQLite rồi mới
  stop/remove VM.
- Cấu hình phần cứng qua `MuMuManager.exe simulation` và thao tác app qua
  `MuMuManager.exe adb`.
- Tìm kiếm/lọc profile, Settings, log viewer, mock backend để chạy UI độc lập.

## Kiến trúc (tóm tắt)
- **Frontend** (`src/`): React + TS + Tailwind. UI **không** gọi thẳng
  `MuMuManager`; mọi tương tác qua interface `Backend` (`src/lib/`) với adapter
  Tauri thật và adapter mock cho trình duyệt.
- **Backend** (`src-tauri/src/`): Rust + Tauri. Tương tác OS đi qua trait
  `EmulatorClient` (`src-tauri/src/emulator/`) và `AdbWorker` (`adb.rs`).
  `profile_ops` điều phối CRUD/run/stop profile; `orchestrator` đảm bảo vòng đời
  create → apply hardware → restore → run → backup → destroy.

## Yêu cầu môi trường
| Công cụ | Ghi chú |
|--------|--------|
| Node ≥ 20 | frontend |
| Rust (stable) + MSVC build tools | backend Tauri — cài qua [rustup](https://rustup.rs) |
| WebView2 Runtime | có sẵn Win11; bundle cho Win10 |
| MuMu Player 12 (mới nhất) | runtime; nếu chưa cài, app tự fallback mock |

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
cd src-tauri && cargo test --features mock-emulator      # test backend không cần MuMu
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
  emulator/             Trait EmulatorClient + parser + MuMu adapter + mock
  adb.rs                Trait AdbWorker + real/mock backup/restore/app ops
  profile_ops.rs        CRUD/run/stop profile
  orchestrator.rs       Disposable VM lifecycle
  db.rs snapshot.rs     SQLite + local snapshot store
  state.rs commands.rs error.rs model.rs
docs/                   Tài liệu kiến trúc & đóng góp
```

## Trạng thái
Local-first đã là hướng chính: SQLite + snapshot store local, giới hạn 5 profile
chạy đồng thời. Server repository, cloud storage, proxy sticky per-account và
lease/heartbeat là hướng mở rộng sau; xem `docs/ARCHITECTURE.md`.
