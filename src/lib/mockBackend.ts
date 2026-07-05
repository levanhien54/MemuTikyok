import type { Backend } from './backend';
import type {
  Instance,
  BulkOperation,
  AppSettings,
  InstanceStatus,
  AccountProfile,
  CreateInstancePayload,
  SnapshotRecord,
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

const SNAP_KEY = 'mpm.snap.v1';

/** Kho snapshot mô phỏng: account_key -> bản ghi mới nhất. */
function loadSnaps(): Record<string, SnapshotRecord> {
  try {
    const raw = localStorage.getItem(SNAP_KEY);
    return raw ? (JSON.parse(raw) as Record<string, SnapshotRecord>) : {};
  } catch {
    return {};
  }
}

function saveSnaps(snaps: Record<string, SnapshotRecord>): void {
  try {
    localStorage.setItem(SNAP_KEY, JSON.stringify(snaps));
  } catch {
    /* bỏ qua */
  }
}

async function sha256Hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
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

/**
 * Backend giả lập cho phát triển UI & test khi chưa có Rust/MEmu.
 * Mô phỏng độ trễ và hành vi bất đồng bộ của memuc (§7.2 SRS).
 *
 * Metadata (note, country, lastLaunchedAt, account) được persist vào localStorage
 * — đóng vai "cơ sở dữ liệu" phía web, sống sót qua reload (bản Tauri dùng SQLite).
 */
const DEFAULT_SETTINGS: AppSettings = {
  memuPath: null,
  pollIntervalMs: 3000,
  maxConcurrency: 3,
  theme: 'dark',
  layout: 'list',
  tiktokApkPath: null,
};

const META_KEY = 'mpm.meta.v1';

interface MetaEntry {
  note: string;
  country: string | null;
  lastLaunchedAt: number | null;
  account: AccountProfile | null;
}

type MetaMap = Record<number, MetaEntry>;

/** Quốc gia mô phỏng "nhận theo IP thực" khi VM chạy. */
const GEO_POOL = ['VN', 'US', 'SG', 'JP', 'TH', 'ID', 'PH', 'KR'];

/** Quốc gia IP thoát mô phỏng của "host" (để test cổng quốc gia khi khởi chạy). */
const MOCK_EGRESS_COUNTRY = 'VN';

function loadMeta(): MetaMap {
  try {
    const raw = localStorage.getItem(META_KEY);
    return raw ? (JSON.parse(raw) as MetaMap) : {};
  } catch {
    return {};
  }
}

function saveMeta(meta: MetaMap): void {
  try {
    localStorage.setItem(META_KEY, JSON.stringify(meta));
  } catch {
    /* localStorage không khả dụng — bỏ qua (best-effort) */
  }
}

/** Tạo & lưu một bản snapshot mô phỏng cho account (dùng lại ở backup & teardown). */
async function makeSnapshot(index: number, accountKey: string): Promise<SnapshotRecord> {
  const payload = `tiktok-data:${accountKey}:${Date.now()}`;
  const record: SnapshotRecord = {
    storageKey: `${accountKey}/${Date.now()}.tar.zst`,
    sha256: await sha256Hex(payload),
    sizeBytes: 3_500_000 + index * 100_000, // ~3.5MB giả lập (đã prune cache)
    apkVersion: 'mock-1.0',
    createdAt: Date.now(),
  };
  const snaps = loadSnaps();
  snaps[accountKey] = record;
  saveSnaps(snaps);
  return record;
}

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

export function createMockBackend(): Backend {
  const meta = loadMeta();
  let settings = { ...DEFAULT_SETTINGS };
  // Profile store (kiến trúc disposable). Seed vài profile demo lần đầu.
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
  const listeners = new Set<(i: Instance[]) => void>();
  const autoDone = new Set<(r: SessionReport) => void>();
  const autoError = new Set<(index: number, message: string) => void>();

  const patchMeta = (index: number, patch: Partial<MetaEntry>) => {
    const prev: MetaEntry = meta[index] ?? {
      note: '',
      country: null,
      lastLaunchedAt: null,
      account: null,
    };
    meta[index] = { ...prev, ...patch };
    saveMeta(meta);
  };

  const applyMeta = (base: Instance): Instance => {
    const m = meta[base.index];
    if (!m) return base;
    return {
      ...base,
      note: m.note ?? base.note,
      country: m.country ?? base.country,
      lastLaunchedAt: m.lastLaunchedAt ?? base.lastLaunchedAt,
      account: m.account ?? base.account,
    };
  };

  function seed(): Instance[] {
    const titles = ['tiktok_minh', 'tiktok_lan', 'farm_a01', 'farm_a02', 'sandbox'];
    const now = Date.now();
    return titles.map((title, index) =>
      applyMeta({
        index,
        title,
        status: (index % 2 === 0 ? 'running' : 'stopped') as InstanceStatus,
        pid: index % 2 === 0 ? 10000 + index : null,
        windowHandle: index % 2 === 0 ? 200000 + index : null,
        ip: index % 2 === 0 ? `192.168.1.${20 + index}` : null,
        diskUsageBytes: (2 + index) * 1024 * 1024 * 1024,
        lastLaunchedAt: index % 2 === 0 ? now - index * 3600_000 - 120_000 : null,
        country: null,
        note: '',
        account: emptyAccount(title),
      }),
    );
  }

  let instances = seed();

  const emit = () => {
    const snapshot = instances.map((i) => ({ ...i }));
    listeners.forEach((cb) => cb(snapshot));
  };

  const transition = (index: number, pending: InstanceStatus, final: InstanceStatus, delay = 900) => {
    instances = instances.map((i) => (i.index === index ? { ...i, status: pending } : i));
    emit();
    setTimeout(() => {
      const running = final === 'running';
      if (running) {
        // Mô phỏng "nhận IP thực → tra quốc gia" và lưu vào DB (nếu chưa có).
        const country = meta[index]?.country ?? GEO_POOL[index % GEO_POOL.length]!;
        const launched = Date.now();
        patchMeta(index, { country, lastLaunchedAt: launched });
      }
      instances = instances.map((i) => {
        if (i.index !== index) return i;
        return {
          ...i,
          status: final,
          pid: running ? 10000 + index : null,
          windowHandle: running ? 200000 + index : null,
          ip: running ? `192.168.1.${20 + index}` : null,
          lastLaunchedAt: running ? (meta[index]?.lastLaunchedAt ?? i.lastLaunchedAt) : i.lastLaunchedAt,
          country: running ? (meta[index]?.country ?? i.country) : i.country,
        };
      });
      emit();
    }, delay);
  };

  return {
    async listInstances() {
      return instances.map((i) => ({ ...i }));
    },
    async startInstance(index) {
      transition(index, 'starting', 'running');
    },
    async stopInstance(index) {
      transition(index, 'stopping', 'stopped');
    },
    async rebootInstance(index) {
      transition(index, 'starting', 'running', 1400);
    },
    async createInstance(payload: CreateInstancePayload) {
      const nextIndex = instances.length ? Math.max(...instances.map((i) => i.index)) + 1 : 0;
      const title = payload.account.tiktokUsername.trim() || `MEmu-${nextIndex}`;
      const account = { ...payload.account, tiktokUsername: title };
      const note = payload.note ?? '';
      const country = payload.country?.trim().toUpperCase() || null;
      patchMeta(nextIndex, { account, note, country, lastLaunchedAt: null });
      instances = [
        ...instances,
        {
          index: nextIndex,
          title,
          status: 'stopped',
          pid: null,
          windowHandle: null,
          ip: null,
          diskUsageBytes: 2 * 1024 * 1024 * 1024,
          lastLaunchedAt: null,
          country,
          note,
          account,
        },
      ];
      emit();
    },
    async updateAccount(index: number, account: AccountProfile) {
      patchMeta(index, { account });
      instances = instances.map((i) =>
        i.index === index ? { ...i, account, title: account.tiktokUsername } : i,
      );
      emit();
    },
    async updateNote(index: number, note: string) {
      patchMeta(index, { note });
      instances = instances.map((i) => (i.index === index ? { ...i, note } : i));
      emit();
    },
    async updateCountry(index: number, country: string | null) {
      const cc = country?.trim().toUpperCase() || null;
      patchMeta(index, { country: cc });
      instances = instances.map((i) => (i.index === index ? { ...i, country: cc } : i));
      emit();
    },
    async removeInstance(index) {
      delete meta[index];
      saveMeta(meta);
      instances = instances.filter((i) => i.index !== index);
      emit();
    },
    async renameInstance(index, title) {
      instances = instances.map((i) => (i.index === index ? { ...i, title } : i));
      emit();
    },
    async bulkAction(operation: BulkOperation, indexes: number[]) {
      const map = {
        start: () => indexes.forEach((i) => transition(i, 'starting', 'running')),
        stop: () => indexes.forEach((i) => transition(i, 'stopping', 'stopped')),
        reboot: () => indexes.forEach((i) => transition(i, 'starting', 'running', 1400)),
      };
      map[operation]();
    },
    async getHardware(index: number) {
      const inst = instances.find((i) => i.index === index);
      return inst ? mockFingerprint(inst.title) : null;
    },
    async installTiktok(_index: number) {
      /* mô phỏng cài TikTok */
    },
    async scanEmulator(_index: number) {
      // Mẫu khớp thực tế MEmu: chỉ còn native-bridge + hypervisor lộ.
      return [
        { check: 'Native Bridge (ARM→x86)', detected: true, detail: 'libnb.so' },
        { check: 'CPU hypervisor flag', detected: true, detail: "cpuinfo có 'hypervisor'" },
        { check: 'ro.kernel.qemu', detected: false, detail: 'rỗng' },
        { check: 'File QEMU/Genymotion', detected: false, detail: 'sạch' },
        { check: 'GPU renderer ảo', detected: false, detail: 'GPU thật (Adreno/Mali)' },
      ];
    },
    async launchInstance(index: number, accountKey: string) {
      // Cổng quốc gia: chỉ chạy khi IP thoát (mô phỏng) khớp quốc gia yêu cầu.
      const expected = meta[index]?.country;
      if (expected && expected.toUpperCase() !== MOCK_EGRESS_COUNTRY) {
        throw new Error(
          `Quốc gia IP thoát (${MOCK_EGRESS_COUNTRY}) không khớp quốc gia yêu cầu (${expected.toUpperCase()}). Không khởi chạy để tránh sai lệch định vị.`,
        );
      }
      // Mô phỏng: nạp fingerprint (đã lưu) + start; trả về true nếu có snapshot để restore.
      transition(index, 'starting', 'running');
      const snaps = loadSnaps();
      return !!snaps[accountKey];
    },
    async backupInstance(index: number, accountKey: string) {
      // Mô phỏng: "trích xuất" dữ liệu app hiện tại của VM và lưu snapshot.
      return makeSnapshot(index, accountKey);
    },
    async restoreInstance(_index: number, accountKey: string) {
      const snaps = loadSnaps();
      const record = snaps[accountKey];
      if (!record) throw new Error('Chưa có snapshot cho tài khoản này');
      return record;
    },
    async getSettings() {
      return { ...settings };
    },
    async saveSettings(next) {
      settings = { ...next };
      return { ...settings };
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
    subscribeInstances(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
