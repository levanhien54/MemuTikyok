import { create } from 'zustand';
import type { AppSettings } from '@/types/instance';
import { getBackend } from '@/lib';
import { toast } from '@/store/useToastStore';

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
    const optimistic = normalizeSettings({ ...current, ...partial });
    set({ settings: optimistic });
    applyTheme(optimistic.theme);
    try {
      const next = await getBackend().saveSettings(optimistic);
      set({ settings: next });
      applyTheme(next.theme);
    } catch (e) {
      set({ settings: current });
      applyTheme(current.theme);
      const message = e instanceof Error ? e.message : String(e);
      toast.error(`Lưu cài đặt lỗi: ${message}`);
      throw e;
    }
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

function normalizeSettings(settings: AppSettings): AppSettings {
  return {
    ...settings,
    pollIntervalMs: clampNumber(settings.pollIntervalMs, 1000, 10000, 3000),
    maxConcurrency: clampNumber(settings.maxConcurrency, 1, 10, 3),
  };
}

function clampNumber(value: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

function applyTheme(theme: AppSettings['theme']) {
  const root = document.documentElement;
  root.classList.toggle('dark', theme === 'dark');
}
