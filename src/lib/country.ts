/**
 * Ánh xạ mã quốc gia ISO 3166-1 alpha-2 sang cờ emoji + tên tiếng Việt.
 * Danh sách gọn cho các quốc gia hay gặp; mở rộng khi cần.
 */
const NAMES: Record<string, string> = {
  VN: 'Việt Nam',
  US: 'Hoa Kỳ',
  GB: 'Anh',
  JP: 'Nhật Bản',
  KR: 'Hàn Quốc',
  SG: 'Singapore',
  TH: 'Thái Lan',
  ID: 'Indonesia',
  PH: 'Philippines',
  MY: 'Malaysia',
  DE: 'Đức',
  FR: 'Pháp',
  CA: 'Canada',
  AU: 'Úc',
  IN: 'Ấn Độ',
  TW: 'Đài Loan',
  HK: 'Hồng Kông',
  MX: 'Mexico',
};

/** Danh sách mã quốc gia hay dùng (cho ô chọn khi tạo VM). */
export const COUNTRY_CODES: string[] = Object.keys(NAMES);

/** Chuyển mã 2 chữ cái thành cờ emoji bằng ký tự Regional Indicator. */
export function countryFlag(code: string): string {
  if (!/^[A-Za-z]{2}$/.test(code)) return '🏳️';
  const cc = code.toUpperCase();
  const A = 0x1f1e6;
  return String.fromCodePoint(A + (cc.charCodeAt(0) - 65), A + (cc.charCodeAt(1) - 65));
}

export function countryName(code: string): string {
  return NAMES[code.toUpperCase()] ?? code.toUpperCase();
}

/** Nhãn hiển thị đầy đủ: "🇻🇳 Việt Nam". `null` → gạch ngang. */
export function countryLabel(code: string | null): string {
  if (!code) return '—';
  return `${countryFlag(code)} ${countryName(code)}`;
}
