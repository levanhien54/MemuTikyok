import { create } from 'zustand';
import type { AccountProfile, ProfileView } from '@/types/instance';
import { getBackend } from '@/lib';
import { toast } from '@/store/useToastStore';

type LoadState = 'idle' | 'loading' | 'ready' | 'error';

interface ProfileState {
  profiles: ProfileView[];
  loadState: LoadState;
  error: string | null;
  search: string;

  init: () => () => void;
  refresh: () => Promise<void>;
  setSearch: (q: string) => void;

  create: (account: AccountProfile, note: string, country: string | null) => Promise<void>;
  update: (
    username: string,
    account: AccountProfile,
    note: string,
    country: string | null,
  ) => Promise<void>;
  run: (username: string) => Promise<number>;
  stop: (username: string) => Promise<void>;
  remove: (username: string) => Promise<void>;
}

export const useProfileStore = create<ProfileState>((set, get) => ({
  profiles: [],
  loadState: 'idle',
  error: null,
  search: '',

  init() {
    void get().refresh();
    // Kết quả phiên automation → toast (dùng chung sự kiện với backend runner).
    const unsub = getBackend().subscribeAutomation(
      (r) => {
        toast.success(
          `Phiên xem xong VM #${r.index}: ${r.videos} video, ${r.liked} like (${Math.round(
            r.durationMs / 1000,
          )}s)`,
        );
        void get().refresh();
      },
      (index, message) => toast.error(`Phiên VM #${index} lỗi: ${message}`),
    );
    return unsub;
  },

  async refresh() {
    try {
      set({ loadState: get().profiles.length ? 'ready' : 'loading' });
      const profiles = await getBackend().listProfiles();
      set({ profiles, loadState: 'ready', error: null });
    } catch (e) {
      set({ loadState: 'error', error: e instanceof Error ? e.message : String(e) });
    }
  },

  setSearch(q) {
    set({ search: q });
  },

  async create(account, note, country) {
    await getBackend().createProfile(account, note, country);
    await get().refresh();
  },
  async update(username, account, note, country) {
    await getBackend().updateProfile(username, account, note, country);
    await get().refresh();
  },
  async run(username) {
    const vm = await getBackend().runProfile(username);
    await get().refresh();
    return vm;
  },
  async stop(username) {
    await getBackend().stopProfile(username);
    await get().refresh();
  },
  async remove(username) {
    await getBackend().deleteProfile(username);
    await get().refresh();
  },
}));
