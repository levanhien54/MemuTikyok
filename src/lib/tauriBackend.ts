import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Backend } from './backend';
import type {
  Instance,
  BulkOperation,
  AppSettings,
  AccountProfile,
  CreateInstancePayload,
  SnapshotRecord,
  HardwareProfile,
  EmulatorTell,
  InstancesUpdateEvent,
} from '@/types/instance';

/**
 * Adapter thật: ánh xạ interface Backend sang lệnh Tauri (`invoke`) và
 * sự kiện đẩy (`listen`) đã định nghĩa ở §8.4 SRS.
 * Tên lệnh khớp với `#[tauri::command]` trong src-tauri/src/commands.rs.
 */
export function createTauriBackend(): Backend {
  return {
    listInstances() {
      return invoke<Instance[]>('list_instances');
    },
    startInstance(index) {
      return invoke('start_instance', { index });
    },
    stopInstance(index) {
      return invoke('stop_instance', { index });
    },
    rebootInstance(index) {
      return invoke('reboot_instance', { index });
    },
    createInstance(payload: CreateInstancePayload) {
      return invoke('create_instance', { payload });
    },
    updateAccount(index: number, account: AccountProfile) {
      return invoke('update_account', { index, account });
    },
    updateNote(index: number, note: string) {
      return invoke('update_note', { index, note });
    },
    updateCountry(index: number, country: string | null) {
      return invoke('update_country', { index, country });
    },
    removeInstance(index) {
      return invoke('remove_instance', { index });
    },
    renameInstance(index, title) {
      return invoke('rename_instance', { index, title });
    },
    bulkAction(operation: BulkOperation, indexes: number[]) {
      return invoke('bulk_action', { operation, indexes });
    },
    launchInstance(index, accountKey) {
      return invoke<boolean>('launch_instance', { index, accountKey });
    },
    getHardware(index) {
      return invoke<HardwareProfile | null>('get_hardware', { index });
    },
    installTiktok(index) {
      return invoke('install_tiktok', { index }).then(() => undefined);
    },
    scanEmulator(index) {
      return invoke<EmulatorTell[]>('scan_emulator', { index });
    },
    backupInstance(index, accountKey) {
      return invoke<SnapshotRecord>('backup_instance', { index, accountKey });
    },
    restoreInstance(index, accountKey) {
      return invoke<SnapshotRecord>('restore_instance', { index, accountKey });
    },
    getSettings() {
      return invoke<AppSettings>('get_settings');
    },
    saveSettings(settings) {
      return invoke<AppSettings>('save_settings', { settings });
    },
    getPoolSize() {
      return invoke<number>('warm_pool_size');
    },
    refillPool(baseIndex, target) {
      return invoke<number>('warm_pool_refill', { baseIndex, target });
    },
    provisionSession(accountKey, hardware) {
      return invoke<number>('provision_session', { accountKey, hardware });
    },
    acquireFromPool(baseIndex, accountKey, hardware) {
      return invoke<number>('warm_pool_acquire', { baseIndex, accountKey, hardware });
    },
    swapAccount(index, accountKey, hardware) {
      return invoke<void>('swap_account', { index, accountKey, hardware });
    },
    teardownSession(index, accountKey) {
      return invoke<SnapshotRecord>('teardown_session', { index, accountKey });
    },
    subscribeInstances(cb) {
      const unlistenPromise = listen<InstancesUpdateEvent>('instances:update', (event) => {
        cb(event.payload.instances);
      });
      return () => {
        void unlistenPromise.then((unlisten) => unlisten());
      };
    },
  };
}
