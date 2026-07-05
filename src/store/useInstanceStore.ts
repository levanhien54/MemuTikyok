import { create } from 'zustand';
import type {
  Instance,
  BulkOperation,
  CreateInstancePayload,
  AccountProfile,
  SnapshotRecord,
  HardwareProfile,
  EmulatorTell,
} from '@/types/instance';
import { getBackend } from '@/lib';
import { toast } from '@/store/useToastStore';

type LoadState = 'idle' | 'loading' | 'ready' | 'error';

interface InstanceState {
  instances: Instance[];
  selected: Set<number>;
  loadState: LoadState;
  error: string | null;
  search: string;

  // lifecycle
  init: () => () => void;
  refresh: () => Promise<void>;

  // selection
  toggleSelect: (index: number) => void;
  selectAll: () => void;
  clearSelection: () => void;
  setSearch: (q: string) => void;

  // actions
  start: (index: number) => Promise<void>;
  stop: (index: number) => Promise<void>;
  reboot: (index: number) => Promise<void>;
  remove: (index: number) => Promise<void>;
  create: (payload: CreateInstancePayload) => Promise<void>;
  updateAccount: (index: number, account: AccountProfile) => Promise<void>;
  updateNote: (index: number, note: string) => Promise<void>;
  updateCountry: (index: number, country: string | null) => Promise<void>;
  /** Một-chạm: chạy VM và nạp session tài khoản. Trả về true nếu đã restore snapshot. */
  launch: (index: number, accountKey: string) => Promise<boolean>;
  backup: (index: number, accountKey: string) => Promise<SnapshotRecord>;
  restore: (index: number, accountKey: string) => Promise<SnapshotRecord>;
  getHardware: (index: number) => Promise<HardwareProfile | null>;
  installTiktok: (index: number) => Promise<void>;
  scanEmulator: (index: number) => Promise<EmulatorTell[]>;
  rename: (index: number, title: string) => Promise<void>;
  bulk: (op: BulkOperation) => Promise<void>;
  /** Chạy phiên "xem feed" giả người ở nền; kết quả toast qua sự kiện automation. */
  runWatchSession: (index: number) => Promise<void>;
}

export const useInstanceStore = create<InstanceState>((set, get) => ({
  instances: [],
  selected: new Set(),
  loadState: 'idle',
  error: null,
  search: '',

  init() {
    const backend = getBackend();
    set({ loadState: 'loading' });

    // Snapshot ban đầu
    backend
      .listInstances()
      .then((instances) => set({ instances, loadState: 'ready', error: null }))
      .catch((e: unknown) =>
        set({ loadState: 'error', error: e instanceof Error ? e.message : String(e) }),
      );

    // Luồng cập nhật realtime (polling ở backend đẩy qua đây)
    const unsubInstances = backend.subscribeInstances((instances) =>
      set({ instances, loadState: 'ready' }),
    );
    // Kết quả phiên automation → toast.
    const unsubAuto = backend.subscribeAutomation(
      (r) =>
        toast.success(
          `Phiên xem xong VM #${r.index}: ${r.videos} video, ${r.liked} like (${Math.round(
            r.durationMs / 1000,
          )}s)`,
        ),
      (index, message) => toast.error(`Phiên VM #${index} lỗi: ${message}`),
    );
    return () => {
      unsubInstances();
      unsubAuto();
    };
  },

  async refresh() {
    try {
      const instances = await getBackend().listInstances();
      set({ instances, loadState: 'ready', error: null });
    } catch (e) {
      set({ loadState: 'error', error: e instanceof Error ? e.message : String(e) });
    }
  },

  toggleSelect(index) {
    const selected = new Set(get().selected);
    if (selected.has(index)) selected.delete(index);
    else selected.add(index);
    set({ selected });
  },
  selectAll() {
    set({ selected: new Set(get().instances.map((i) => i.index)) });
  },
  clearSelection() {
    set({ selected: new Set() });
  },
  setSearch(q) {
    set({ search: q });
  },

  async start(index) {
    await getBackend().startInstance(index);
  },
  async stop(index) {
    await getBackend().stopInstance(index);
  },
  async reboot(index) {
    await getBackend().rebootInstance(index);
  },
  async remove(index) {
    await getBackend().removeInstance(index);
    const selected = new Set(get().selected);
    selected.delete(index);
    set({ selected });
  },
  async create(payload) {
    await getBackend().createInstance(payload);
  },
  async updateAccount(index, account) {
    await getBackend().updateAccount(index, account);
  },
  async updateNote(index, note) {
    await getBackend().updateNote(index, note);
  },
  async updateCountry(index, country) {
    await getBackend().updateCountry(index, country);
  },
  async launch(index, accountKey) {
    // Nạp lại fingerprint đã lưu + start + restore session (backend lo toàn bộ).
    return getBackend().launchInstance(index, accountKey);
  },
  async backup(index, accountKey) {
    return getBackend().backupInstance(index, accountKey);
  },
  async restore(index, accountKey) {
    return getBackend().restoreInstance(index, accountKey);
  },
  async getHardware(index) {
    return getBackend().getHardware(index);
  },
  async installTiktok(index) {
    return getBackend().installTiktok(index);
  },
  async scanEmulator(index) {
    return getBackend().scanEmulator(index);
  },
  async rename(index, title) {
    await getBackend().renameInstance(index, title);
  },
  async bulk(op) {
    const indexes = [...get().selected];
    if (indexes.length === 0) return;
    await getBackend().bulkAction(op, indexes);
  },
  async runWatchSession(index) {
    await getBackend().runWatchSession(index);
  },
}));
