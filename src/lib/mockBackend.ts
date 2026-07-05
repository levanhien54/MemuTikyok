import type { Backend } from './backend';
import type {
  AppSettings,
  AccountProfile,
  HardwareProfile,
  SessionReport,
  Profile,
  ProfileView,
} from '@/types/instance';

/** Sinh fingerprint mô phỏng, tất định theo tên tài khoản (cho UI demo). */
function mockFingerprint(title: string): HardwareProfile {
  // Bộ device NHẤT QUÁN (model/brand/device khớp buildFingerprint) — khớp bảng Rust.
  const devices = [
    {
      model: 'SM-N935F', brand: 'samsung', manufacturer: 'samsung', device: 'gracerlte',
      buildFingerprint: 'samsung/gracerltexx/gracerlte:8.0.0/R16NW/N935FXXS4BRK2:user/release-keys',
      resWidth: 1080, resHeight: 1920, dpi: 480,
    },
    {
      model: 'SM-G960F', brand: 'samsung', manufacturer: 'samsung', device: 'starlte',
      buildFingerprint: 'samsung/starltexx/starlte:10/QP1A.190711.020/G960FXXUFFUJ1:user/release-keys',
      resWidth: 1080, resHeight: 2220, dpi: 480,
    },
    {
      model: 'Redmi Note 8', brand: 'Redmi', manufacturer: 'Xiaomi', device: 'ginkgo',
      buildFingerprint: 'Redmi/ginkgo/ginkgo:11/RP1A.200720.011/V12.5.1.0.RCOMIXM:user/release-keys',
      resWidth: 1080, resHeight: 2340, dpi: 440,
    },
  ];
  let h = 0;
  for (const ch of title) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  const d = devices[h % devices.length]!;
  const imei = (h.toString() + '000000000000000').slice(0, 15);
  const androidId = (h.toString(16) + '0000000000000000').slice(0, 16);
  return { ...d, imei, androidId, mac: '02:11:22:33:44:55' };
}

function emptyAccount(username: string): AccountProfile {
  return {
    tiktokUsername: username,
    tiktokPassword: '',
    twoFa: '',
    tiktokPasskey: '',
    email: '',
    emailPassword: '',
  };
}

const DEFAULT_SETTINGS: AppSettings = {
  memuPath: null,
  pollIntervalMs: 3000,
  maxConcurrency: 3,
  theme: 'dark',
  layout: 'list',
  tiktokApkPath: null,
};

/** Quốc gia IP thoát mô phỏng của "host" (để test cổng quốc gia khi Chạy profile). */
const MOCK_EGRESS_COUNTRY = 'VN';

const PROFILES_KEY = 'mpm.profiles.v1';
function loadProfiles(): Record<string, Profile> {
  try {
    const raw = localStorage.getItem(PROFILES_KEY);
    return raw ? (JSON.parse(raw) as Record<string, Profile>) : {};
  } catch {
    return {};
  }
}
function saveProfiles(p: Record<string, Profile>): void {
  try {
    localStorage.setItem(PROFILES_KEY, JSON.stringify(p));
  } catch {
    /* bỏ qua */
  }
}

/**
 * Backend giả lập cho phát triển UI & test khi chưa có Rust/MEmu.
 * Kiến trúc DISPOSABLE: chỉ vòng đời PROFILE. Profile persist vào localStorage —
 * đóng vai "cơ sở dữ liệu" phía web, sống sót qua reload (bản Tauri dùng SQLite).
 */
export function createMockBackend(): Backend {
  let settings = { ...DEFAULT_SETTINGS };
  // Seed vài profile demo lần đầu.
  const profiles = loadProfiles();
  if (Object.keys(profiles).length === 0) {
    for (const u of ['tiktok_minh', 'tiktok_lan', 'farm_a01']) {
      profiles[u] = {
        username: u,
        account: emptyAccount(u),
        hardware: mockFingerprint(u),
        country: null,
        note: '',
        createdAt: Date.now(),
        lastRunAt: null,
      };
    }
    saveProfiles(profiles);
  }
  const runningProfiles: Record<string, number> = {};
  let vmCounter = 100;
  const autoDone = new Set<(r: SessionReport) => void>();
  const autoError = new Set<(index: number, message: string) => void>();

  return {
    async createProfile(account, note, country) {
      const username = account.tiktokUsername.trim();
      if (!username) throw new Error('Tên tài khoản không được rỗng');
      if (profiles[username]) throw new Error('Profile tên này đã tồn tại');
      profiles[username] = {
        username,
        account: { ...account, tiktokUsername: username },
        hardware: mockFingerprint(username),
        country: country?.trim().toUpperCase() || null,
        note: note.trim(),
        createdAt: Date.now(),
        lastRunAt: null,
      };
      saveProfiles(profiles);
      return username;
    },
    async listProfiles(): Promise<ProfileView[]> {
      return Object.values(profiles)
        .sort((a, b) => a.createdAt - b.createdAt)
        .map((p) => ({ profile: p, runningVm: runningProfiles[p.username] ?? null }));
    },
    async updateProfile(username, account, note, country) {
      const p = profiles[username];
      if (!p) throw new Error('Không tìm thấy profile');
      p.account = { ...account, tiktokUsername: username };
      p.note = note.trim();
      p.country = country?.trim().toUpperCase() || null;
      saveProfiles(profiles);
    },
    async runProfile(username) {
      if (runningProfiles[username] != null) return runningProfiles[username]!;
      // Cổng quốc gia (khớp backend thật): IP thoát mô phỏng phải khớp quốc gia yêu cầu.
      const expected = profiles[username]?.country;
      if (expected && expected.toUpperCase() !== MOCK_EGRESS_COUNTRY) {
        throw new Error(
          `Quốc gia IP thoát (${MOCK_EGRESS_COUNTRY}) không khớp quốc gia yêu cầu (${expected.toUpperCase()}). Không khởi chạy để tránh sai lệch định vị.`,
        );
      }
      if (Object.keys(runningProfiles).length >= 5)
        throw new Error('Đã đạt tối đa 5 VM chạy đồng thời — dừng bớt profile khác');
      const vm = vmCounter++;
      runningProfiles[username] = vm;
      if (profiles[username]) {
        profiles[username].lastRunAt = Date.now();
        saveProfiles(profiles);
      }
      return vm;
    },
    async stopProfile(username) {
      if (runningProfiles[username] == null) return null;
      delete runningProfiles[username];
      return {
        storageKey: `${username}/${Date.now()}.tar.zst`,
        sha256: 'mock',
        sizeBytes: 3_500_000,
        apkVersion: 'mock-1.0',
        createdAt: Date.now(),
      };
    },
    async deleteProfile(username) {
      delete profiles[username];
      delete runningProfiles[username];
      saveProfiles(profiles);
    },
    async scanEmulator(_index: number) {
      // Mẫu khớp thực tế MEmu: chỉ còn native-bridge + hypervisor lộ.
      return [
        { check: 'Native Bridge (ARM→x86)', detected: true, detail: 'libnb.so' },
        { check: 'CPU hypervisor flag', detected: true, detail: "cpuinfo có 'hypervisor'" },
        { check: 'ro.kernel.qemu', detected: false, detail: 'rỗng' },
        { check: 'File QEMU/Genymotion', detected: false, detail: 'sạch' },
        { check: 'GPU renderer ảo', detected: false, detail: 'GPU thật (Adreno/Mali)' },
        {
          check: 'Magisk/resetprop (khóa model)',
          detected: true,
          detail: 'THIẾU — model bị MEmu ghi đè (cần Magisk trong base image)',
        },
      ];
    },
    async runWatchSession(index: number) {
      // Mô phỏng: sau ~1.5s phát báo cáo phiên (bản thật chạy nền vài phút).
      const videos = 5 + Math.floor(Math.random() * 8);
      const liked = Math.floor(videos * 0.12);
      setTimeout(() => {
        const report: SessionReport = { index, videos, liked, durationMs: videos * 9000 };
        autoDone.forEach((cb) => cb(report));
      }, 1500);
    },
    subscribeAutomation(onDone, onError) {
      autoDone.add(onDone);
      autoError.add(onError);
      return () => {
        autoDone.delete(onDone);
        autoError.delete(onError);
      };
    },
    async getSettings() {
      return { ...settings };
    },
    async saveSettings(next) {
      settings = { ...next };
      return { ...settings };
    },
    async getLogs() {
      const t = new Date().toISOString();
      return [
        `${t}  INFO  mpm: Mock backend — log demo (bản Tauri hiện log thật)`,
        `${t}  INFO  mpm::profile_ops: Reconcile khởi động: không có VM mồ côi`,
        `${t}  WARN  mpm::adb: (demo) android_id có thể bị GMS ghi đè sau cài TikTok`,
      ];
    },
  };
}
