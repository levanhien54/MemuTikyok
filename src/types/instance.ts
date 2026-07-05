/**
 * Kiểu dữ liệu chia sẻ FE↔BE (§8.5 SRS).
 * Ở bản production, các kiểu này nên được sinh tự động từ Rust (ts-rs)
 * để tránh lệch schema. Hiện khai báo tay và giữ khớp với `src-tauri/src/model.rs`.
 */

export type InstanceStatus =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'error';

/**
 * Hồ sơ tài khoản TikTok gắn với một VM (MPM tự quản, memuc không biết).
 * ⚠️ NHẠY CẢM: chứa mật khẩu/2FA/passkey. KHÔNG log; khi persist phải mã hóa
 * (DPAPI — SEC-3 §9 SRS). UI luôn che mật khẩu mặc định.
 */
export interface AccountProfile {
  /** Tên tài khoản TikTok — cũng dùng làm tên máy ảo (title). */
  tiktokUsername: string;
  tiktokPassword: string;
  /** Khóa/secret 2FA. */
  twoFa: string;
  tiktokPasskey: string;
  email: string;
  emailPassword: string;
}

export interface Instance {
  /** Chỉ số VM của memuc (định danh chính). */
  index: number;
  title: string;
  status: InstanceStatus;
  pid: number | null;
  windowHandle: number | null;
  ip: string | null;
  diskUsageBytes: number | null;
  /** Thời điểm khởi chạy gần nhất (epoch ms). null nếu chưa từng chạy. */
  lastLaunchedAt: number | null;
  /** Mã quốc gia ISO 3166-1 alpha-2 (vd "VN"), nhận theo IP thực khi chạy. null nếu chưa rõ. */
  country: string | null;
  /** Ghi chú tự do của người dùng. */
  note: string;
  /** Hồ sơ tài khoản (MPM tự quản). */
  account: AccountProfile | null;
}

/** Fingerprint thiết bị gắn với tài khoản (lưu DB, áp khi khởi chạy). */
export interface HardwareProfile {
  model: string;
  brand: string;
  manufacturer: string;
  imei: string;
  androidId: string;
  mac: string;
  resWidth: number;
  resHeight: number;
  dpi: number;
  /** Codename thiết bị (ro.product.device). Optional cho hồ sơ cũ. */
  device?: string;
  /** ro.build.fingerprint của thiết bị thật, nhất quán với model. Optional cho hồ sơ cũ. */
  buildFingerprint?: string;
}

/** Profile = tài khoản (dữ liệu bền), tách khỏi VM (kiến trúc disposable). */
export interface Profile {
  username: string;
  account: AccountProfile;
  hardware: HardwareProfile;
  country: string | null;
  note: string;
  createdAt: number;
  lastRunAt: number | null;
}

/** Profile + trạng thái runtime (đang chạy trên VM nào). */
export interface ProfileView {
  profile: Profile;
  /** vm_index đang chạy (null = idle). */
  runningVm: number | null;
}

/** Payload tạo VM kèm hồ sơ tài khoản + ghi chú. */
export interface CreateInstancePayload {
  account: AccountProfile;
  note: string;
  /** Quốc gia yêu cầu (ISO alpha-2, vd "VN"). Rỗng = không ràng buộc khi chạy. */
  country: string | null;
}

/** Một dấu vết emulator được scan (chống phát hiện). */
export interface EmulatorTell {
  check: string;
  detected: boolean;
  detail: string;
}

/** Bản ghi snapshot (backup) trả về từ backend. */
export interface SnapshotRecord {
  storageKey: string;
  sha256: string;
  sizeBytes: number;
  apkVersion: string | null;
  createdAt: number;
}

/** Báo cáo một phiên automation "xem feed" giả người. */
export interface SessionReport {
  index: number;
  videos: number;
  liked: number;
  durationMs: number;
}

export type BulkOperation = 'start' | 'stop' | 'reboot';

export interface ActionProgress {
  operation: BulkOperation;
  total: number;
  completed: number;
  failed: number;
}

export interface AppSettings {
  memuPath: string | null;
  pollIntervalMs: number;
  maxConcurrency: number;
  theme: 'dark' | 'light';
  layout: 'grid' | 'list';
  /** Đường dẫn APK TikTok (null = dùng mặc định). */
  tiktokApkPath: string | null;
}
