import { isTauri, type Backend } from './backend';
import { createTauriBackend } from './tauriBackend';
import { createMockBackend } from './mockBackend';

/**
 * Chọn adapter phù hợp môi trường: Tauri thật khi có bridge IPC,
 * ngược lại dùng mock (chạy frontend độc lập trong trình duyệt).
 */
let instance: Backend | null = null;

export function getBackend(): Backend {
  if (!instance) {
    instance = isTauri() ? createTauriBackend() : createMockBackend();
  }
  return instance;
}

export type { Backend };
