import type {
  AppSettings,
  AccountProfile,
  SnapshotRecord,
  EmulatorTell,
  SessionReport,
  ProfileView,
} from '@/types/instance';

/**
 * Hợp đồng (contract) giữa UI và backend.
 *
 * UI KHÔNG bao giờ gọi thẳng `invoke`/`MuMuManager`. Mọi tương tác đi qua interface này
 * để: (1) test được bằng mock; (2) chạy frontend độc lập trong trình duyệt khi
 * chưa có Rust; (3) dễ thay đổi khi backend đổi. Tương ứng nguyên tắc adapter §8 SRS.
 *
 * Kiến trúc DISPOSABLE: chỉ vòng đời PROFILE + tiện ích trên VM đang chạy + cài đặt.
 */
export interface Backend {
  // ── Vòng đời PROFILE (profile = dữ liệu bền; VM tạo mới mỗi lần chạy rồi hủy) ──
  /** Tạo profile mới (CHỈ ghi dữ liệu, KHÔNG tạo VM). Trả username. */
  createProfile(account: AccountProfile, note: string, country: string | null): Promise<string>;
  /** Danh sách profile + trạng thái runtime (đang chạy VM nào). */
  listProfiles(): Promise<ProfileView[]>;
  updateProfile(
    username: string,
    account: AccountProfile,
    note: string,
    country: string | null,
  ): Promise<void>;
  /** Chạy profile: cấp VM sạch + cài TikTok + restore session + mở app. Trả vm_index. */
  runProfile(username: string): Promise<number>;
  /** Dừng profile: backup session → hủy VM. Trả snapshot nếu có. */
  stopProfile(username: string): Promise<SnapshotRecord | null>;
  deleteProfile(username: string): Promise<void>;

  // ── Tiện ích trên VM đang chạy của profile ──
  /** Scan dấu vết emulator của VM (chống phát hiện MÁY ẢO). */
  scanEmulator(index: number): Promise<EmulatorTell[]>;
  /** Chạy phiên xem TikTok ở NỀN cho VM. Kết quả về qua subscribeAutomation. */
  runWatchSession(index: number): Promise<void>;
  /** Đưa file video/ảnh vào thư viện máy ảo để đăng. */
  uploadVideoToVm(index: number, localPath: string): Promise<void>;
  /** Đăng ký nhận kết quả phiên automation (done/error). Trả về hàm hủy. */
  subscribeAutomation(
    onDone: (report: SessionReport) => void,
    onError: (index: number, message: string) => void,
  ): () => void;

  // ── Cài đặt + chẩn đoán ──
  getSettings(): Promise<AppSettings>;
  saveSettings(settings: AppSettings): Promise<AppSettings>;
  /** Log ứng dụng gần nhất (ring buffer) — cho LogsView chẩn đoán khi Chạy lỗi. */
  getLogs(): Promise<string[]>;
}

/** True khi đang chạy bên trong Tauri (có bridge IPC). */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
