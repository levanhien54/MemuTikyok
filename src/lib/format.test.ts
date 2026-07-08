import { describe, expect, it } from 'vitest';
import { formatBytes, formatRelativeTime } from './format';

describe('formatBytes', () => {
  it('returns a dash for null', () => {
    expect(formatBytes(null)).toBe('—');
  });

  it('returns 0 B for zero', () => {
    expect(formatBytes(0)).toBe('0 B');
  });

  it('formats byte units', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB');
    expect(formatBytes(2.5 * 1024 * 1024 * 1024)).toBe('2.5 GB');
  });

  it('drops decimals for values greater than or equal to 10', () => {
    expect(formatBytes(15 * 1024)).toBe('15 KB');
  });
});

describe('formatRelativeTime', () => {
  const now = 1_000_000_000_000;

  it('returns idle text for null', () => {
    expect(formatRelativeTime(null, now)).toBe('Chưa chạy');
  });

  it('returns just now for values under one minute', () => {
    expect(formatRelativeTime(now - 30_000, now)).toBe('Vừa xong');
  });

  it('formats minutes, hours, and days', () => {
    expect(formatRelativeTime(now - 5 * 60_000, now)).toBe('5 phút trước');
    expect(formatRelativeTime(now - 3 * 3600_000, now)).toBe('3 giờ trước');
    expect(formatRelativeTime(now - 2 * 86400_000, now)).toBe('2 ngày trước');
  });
});
