import { describe, it, expect } from 'vitest';
import { formatBytes, statusMeta, formatRelativeTime } from './format';

describe('formatBytes', () => {
  it('trả về "—" khi null', () => {
    expect(formatBytes(null)).toBe('—');
  });

  it('trả về "0 B" khi bằng 0', () => {
    expect(formatBytes(0)).toBe('0 B');
  });

  it('định dạng byte đúng đơn vị', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB');
    expect(formatBytes(2.5 * 1024 * 1024 * 1024)).toBe('2.5 GB');
  });

  it('bỏ phần thập phân khi >= 10', () => {
    expect(formatBytes(15 * 1024)).toBe('15 KB');
  });
});

describe('formatRelativeTime', () => {
  const now = 1_000_000_000_000;
  it('trả về "Chưa chạy" khi null', () => {
    expect(formatRelativeTime(null, now)).toBe('Chưa chạy');
  });
  it('dưới 1 phút là "Vừa xong"', () => {
    expect(formatRelativeTime(now - 30_000, now)).toBe('Vừa xong');
  });
  it('tính phút/giờ/ngày', () => {
    expect(formatRelativeTime(now - 5 * 60_000, now)).toBe('5 phút trước');
    expect(formatRelativeTime(now - 3 * 3600_000, now)).toBe('3 giờ trước');
    expect(formatRelativeTime(now - 2 * 86400_000, now)).toBe('2 ngày trước');
  });
});

describe('statusMeta', () => {
  it('ánh xạ mọi trạng thái sang nhãn', () => {
    expect(statusMeta('running').label).toBe('Running');
    expect(statusMeta('stopped').label).toBe('Stopped');
    expect(statusMeta('error').label).toBe('Error');
    expect(statusMeta('starting').label).toContain('Starting');
    expect(statusMeta('stopping').label).toContain('Stopping');
  });
});
