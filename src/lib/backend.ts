import type {
  Instance,
  BulkOperation,
  AppSettings,
  AccountProfile,
  CreateInstancePayload,
  SnapshotRecord,
  HardwareProfile,
  EmulatorTell,
} from '@/types/instance';

/**
 * Hợp đồng (contract) giữa UI và backend.
 *
 * UI KHÔNG bao giờ gọi thẳng `invoke`/`memuc`. Mọi tương tác đi qua interface này
 * để: (1) test được bằng mock; (2) chạy frontend độc lập trong trình duyệt khi
 * chưa có Rust; (3) dễ thay đổi khi backend đổi. Tương ứng nguyên tắc adapter §8 SRS.
 */
export interface Backend {
  listInstances(): Promise<Instance[]>;
  startInstance(index: number): Promise<void>;
  stopInstance(index: number): Promise<void>;
  rebootInstance(index: number): Promise<void>;
  createInstance(payload: CreateInstancePayload): Promise<void>;
  /** Cập nhật hồ sơ tài khoản của một VM. */
  updateAccount(index: number, account: AccountProfile): Promise<void>;
  /** Cập nhật ghi chú của một VM (persist vào CSDL). */
  updateNote(index: number, note: string): Promise<void>;
  /** Cập nhật quốc gia yêu cầu (gate khi khởi chạy). null = bỏ ràng buộc. */
  updateCountry(index: number, country: string | null): Promise<void>;

  /** Lấy fingerprint đã lưu của một VM (để hiển thị). */
  getHardware(index: number): Promise<HardwareProfile | null>;
  /** Cài TikTok APK vào VM. */
  installTiktok(index: number): Promise<void>;
  /** Scan dấu vết emulator của VM (chống phát hiện). */
  scanEmulator(index: number): Promise<EmulatorTell[]>;
  removeInstance(index: number): Promise<void>;
  renameInstance(index: number, title: string): Promise<void>;
  bulkAction(operation: BulkOperation, indexes: number[]): Promise<void>;

  /** Khởi chạy VM: nạp lại fingerprint đã lưu & áp → start → restore. Trả về true nếu đã restore session. */
  launchInstance(index: number, accountKey: string): Promise<boolean>;
  /** Backup dữ liệu phiên TikTok của VM lên kho (theo account_key). */
  backupInstance(index: number, accountKey: string): Promise<SnapshotRecord>;
  /** Nạp snapshot mới nhất của account_key vào VM. */
  restoreInstance(index: number, accountKey: string): Promise<SnapshotRecord>;

  getSettings(): Promise<AppSettings>;
  saveSettings(settings: AppSettings): Promise<AppSettings>;

  /** Số VM đang nóng trong warm pool. */
  getPoolSize(): Promise<number>;
  /** Nạp warm pool tới `target` VM (clone từ `baseIndex`). Trả về số VM trong pool. */
  refillPool(baseIndex: number, target: number): Promise<number>;

  // ── Vòng đời "môi trường dùng-một-lần" (disposable) — §kiến trúc SRS ──
  /** Provision VM sạch cho tài khoản: áp fingerprint + restore session. Trả về vm_index. */
  provisionSession(accountKey: string, hardware: HardwareProfile): Promise<number>;
  /** Lấy nhanh 1 VM đã nóng từ pool rồi swap tài khoản vào. Trả về vm_index. */
  acquireFromPool(baseIndex: number, accountKey: string, hardware: HardwareProfile): Promise<number>;
  /** Swap tài khoản trên VM đang chạy: flash sạch + nạp fingerprint + restore (khởi chạy nhanh). */
  swapAccount(index: number, accountKey: string, hardware: HardwareProfile): Promise<void>;
  /** Kết thúc phiên: backup session rồi hủy VM (disposable). Trả về snapshot vừa backup. */
  teardownSession(index: number, accountKey: string): Promise<SnapshotRecord>;

  /** Đăng ký nhận cập nhật danh sách theo thời gian thực. Trả về hàm hủy đăng ký. */
  subscribeInstances(cb: (instances: Instance[]) => void): () => void;
}

/** True khi đang chạy bên trong Tauri (có bridge IPC). */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
