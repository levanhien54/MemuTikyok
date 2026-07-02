import { create } from 'zustand';
import type { AppSettings } from '@/types/instance';
import { getBackend } from '@/lib';

interface SettingsState {
  settings: AppSettings | null;
  load: () => Promise<void>;
  save: (partial: Partial<AppSettings>) => Promise<void>;
  toggleTheme: () => Promise<void>;
  setLayout: (layout: AppSettings['layout']) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  async load() {
    const settings = await getBackend().getSettings();
    set({ settings });
    applyTheme(settings.theme);
  },
  async save(partial) {
    const current = get().settings;
    if (!current) return;
    const next = await getBackend().saveSettings({ ...current, ...partial });
    set({ settings: next });
    applyTheme(next.theme);
  },
  async toggleTheme() {
    const current = get().settings;
    if (!current) return;
    await get().save({ theme: current.theme === 'dark' ? 'light' : 'dark' });
  },
  async setLayout(layout) {
    await get().save({ layout });
  },
}));

function applyTheme(theme: AppSettings['theme']) {
  const root = document.documentElement;
  root.classList.toggle('dark', theme === 'dark');
}
