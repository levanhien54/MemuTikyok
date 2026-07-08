/**
 * Types shared by the React UI and the Rust/Tauri backend.
 *
 * The current public contract is profile-centric. Raw emulator instances are
 * internal to the backend and are not exposed to the UI.
 */

export interface AccountProfile {
  tiktokUsername: string;
  tiktokPassword: string;
  twoFa: string;
  tiktokPasskey: string;
  email: string;
  emailPassword: string;
}

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
  /** Optional for profiles created before these fields existed. */
  device?: string;
  buildFingerprint?: string;
}

export interface Profile {
  username: string;
  account: AccountProfile;
  hardware: HardwareProfile;
  country: string | null;
  note: string;
  createdAt: number;
  lastRunAt: number | null;
}

export interface ProfileView {
  profile: Profile;
  runningVm: number | null;
}

export interface EmulatorTell {
  check: string;
  detected: boolean;
  detail: string;
}

export interface SnapshotRecord {
  storageKey: string;
  sha256: string;
  sizeBytes: number;
  apkVersion: string | null;
  createdAt: number;
}

export interface SessionReport {
  index: number;
  videos: number;
  liked: number;
  durationMs: number;
}

export interface AppSettings {
  mumuPath: string | null;
  pollIntervalMs: number;
  maxConcurrency: number;
  theme: 'dark' | 'light';
  layout: 'grid' | 'list';
  tiktokApkPath: string | null;
  magiskApkPath: string | null;
}
