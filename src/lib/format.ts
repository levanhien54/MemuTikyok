import type { InstanceStatus } from '@/types/instance';

/**
 * Thời gian khởi chạy gần nhất dạng tương đối (vd "5 phút trước").
 * `now` cho phép inject để test; mặc định Date.now().
 */
export function formatRelativeTime(
  epochMs: number | null,
  now: number = Date.now(),
): string {
  if (epochMs == null) return 'Chưa chạy';
  const diffSec = Math.floor((now - epochMs) / 1000);
  if (diffSec < 0) return 'Vừa xong';
  if (diffSec < 60) return 'Vừa xong';
  const min = Math.floor(diffSec / 60);
  if (min < 60) return `${min} phút trước`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr} giờ trước`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day} ngày trước`;
  return new Date(epochMs).toLocaleDateString('vi-VN');
}

export function formatBytes(bytes: number | null): string {
  if (bytes == null) return '—';
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(value >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}

interface StatusMeta {
  label: string;
  /** Tailwind classes cho dot + text. */
  dot: string;
  text: string;
}

export function statusMeta(status: InstanceStatus): StatusMeta {
  switch (status) {
    case 'running':
      return { label: 'Running', dot: 'bg-success', text: 'text-success' };
    case 'starting':
      return { label: 'Starting…', dot: 'bg-warning animate-pulse', text: 'text-warning' };
    case 'stopping':
      return { label: 'Stopping…', dot: 'bg-warning animate-pulse', text: 'text-warning' };
    case 'error':
      return { label: 'Error', dot: 'bg-danger', text: 'text-danger' };
    case 'stopped':
    default:
      return { label: 'Stopped', dot: 'bg-fg-muted', text: 'text-fg-muted' };
  }
}
