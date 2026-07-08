import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Backend } from './backend';
import type {
  AppSettings,
  SnapshotRecord,
  EmulatorTell,
  SessionReport,
  ProfileView,
  RunProfileResult,
} from '@/types/instance';

/**
 * Adapter thật: ánh xạ interface Backend sang lệnh Tauri (`invoke`) và sự kiện đẩy
 * (`listen`). Tên lệnh khớp với `#[tauri::command]` trong src-tauri/src/commands.rs.
 */
export function createTauriBackend(): Backend {
  return {
    createProfile(account, note, country) {
      return invoke<string>('create_profile', { account, note, country });
    },
    listProfiles() {
      return invoke<ProfileView[]>('list_profiles');
    },
    updateProfile(username, account, note, country) {
      return invoke<void>('update_profile', { username, account, note, country });
    },
    runProfile(username) {
      return invoke<RunProfileResult>('run_profile', { username });
    },
    stopProfile(username) {
      return invoke<SnapshotRecord | null>('stop_profile', { username });
    },
    deleteProfile(username) {
      return invoke<void>('delete_profile', { username });
    },
    scanEmulator(index) {
      return invoke<EmulatorTell[]>('scan_emulator', { index });
    },
    runWatchSession(index) {
      return invoke<void>('run_watch_session', { index });
    },
    uploadVideoToVm(index, localPath) {
      return invoke<void>('upload_video_to_vm', { index, localPath });
    },
    subscribeAutomation(onDone, onError) {
      const pDone = listen<SessionReport>('automation:done', (e) => onDone(e.payload));
      const pErr = listen<{ index: number; error: string }>('automation:error', (e) =>
        onError(e.payload.index, e.payload.error),
      );
      return () => {
        void pDone.then((u) => u());
        void pErr.then((u) => u());
      };
    },
    getSettings() {
      return invoke<AppSettings>('get_settings');
    },
    saveSettings(settings) {
      return invoke<AppSettings>('save_settings', { settings });
    },
    getLogs() {
      return invoke<string[]>('get_logs');
    },
  };
}
