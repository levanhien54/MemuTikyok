import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSettings } from '@/types/instance';

const backendMocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
}));

vi.mock('@/lib', () => ({
  getBackend: () => ({
    getSettings: backendMocks.getSettings,
    saveSettings: backendMocks.saveSettings,
  }),
}));

import { useSettingsStore } from './useSettingsStore';

function settings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    mumuPath: null,
    pollIntervalMs: 3000,
    maxConcurrency: 3,
    theme: 'dark',
    layout: 'list',
    tiktokApkPath: null,
    magiskApkPath: null,
    ...overrides,
  };
}

describe('useSettingsStore', () => {
  beforeEach(() => {
    backendMocks.getSettings.mockReset();
    backendMocks.saveSettings.mockReset();
    backendMocks.saveSettings.mockImplementation(async (next: AppSettings) => next);
    useSettingsStore.setState({ settings: null });
    document.documentElement.className = '';
  });

  afterEach(() => {
    vi.clearAllMocks();
    document.documentElement.className = '';
  });

  it('loads settings and applies the saved theme', async () => {
    backendMocks.getSettings.mockResolvedValue(settings({ theme: 'dark' }));

    await useSettingsStore.getState().load();

    expect(backendMocks.getSettings).toHaveBeenCalledTimes(1);
    expect(useSettingsStore.getState().settings?.theme).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('saves a partial update merged with the current settings', async () => {
    useSettingsStore.setState({ settings: settings({ theme: 'dark', pollIntervalMs: 3000 }) });

    await useSettingsStore.getState().save({ theme: 'light', pollIntervalMs: 1500 });

    expect(backendMocks.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ theme: 'light', pollIntervalMs: 1500, maxConcurrency: 3 }),
    );
    expect(useSettingsStore.getState().settings?.theme).toBe('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });

  it('does not save before settings have loaded', async () => {
    await useSettingsStore.getState().save({ theme: 'light' });

    expect(backendMocks.saveSettings).not.toHaveBeenCalled();
  });

  it('toggles theme through save', async () => {
    useSettingsStore.setState({ settings: settings({ theme: 'dark' }) });

    await useSettingsStore.getState().toggleTheme();

    expect(backendMocks.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ theme: 'light' }),
    );
  });
});
