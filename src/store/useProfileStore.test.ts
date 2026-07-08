import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSettings } from '@/types/instance';

const backendMocks = vi.hoisted(() => ({
  listProfiles: vi.fn(),
  subscribeAutomation: vi.fn(),
}));

vi.mock('@/lib', () => ({
  getBackend: () => ({
    listProfiles: backendMocks.listProfiles,
    subscribeAutomation: backendMocks.subscribeAutomation,
  }),
}));

import { useProfileStore } from './useProfileStore';
import { useSettingsStore } from './useSettingsStore';

function settings(pollIntervalMs: number): AppSettings {
  return {
    mumuPath: null,
    pollIntervalMs,
    maxConcurrency: 3,
    theme: 'dark',
    layout: 'list',
    tiktokApkPath: null,
    magiskApkPath: null,
  };
}

describe('useProfileStore polling', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    backendMocks.listProfiles.mockResolvedValue([]);
    backendMocks.subscribeAutomation.mockReturnValue(vi.fn());
    useProfileStore.setState({ profiles: [], loadState: 'idle', error: null, search: '' });
    useSettingsStore.setState({ settings: settings(1000) });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('uses pollIntervalMs from settings and restarts polling when it changes', async () => {
    const cleanup = useProfileStore.getState().init();

    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(999);
    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1);
    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(2);

    useSettingsStore.setState({ settings: settings(250) });

    await vi.advanceTimersByTimeAsync(249);
    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(2);

    await vi.advanceTimersByTimeAsync(1);
    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(3);

    cleanup();
    await vi.advanceTimersByTimeAsync(1000);
    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(3);
  });

  it('clamps very small poll intervals to 250ms', async () => {
    useSettingsStore.setState({ settings: settings(1) });
    const cleanup = useProfileStore.getState().init();

    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(249);
    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1);
    expect(backendMocks.listProfiles).toHaveBeenCalledTimes(2);

    cleanup();
  });
});
