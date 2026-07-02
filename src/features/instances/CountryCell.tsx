import { useEffect, useRef, useState } from 'react';
import { cn } from '@/lib/cn';
import { COUNTRY_CODES, countryLabel } from '@/lib/country';

/**
 * Ô "Quốc gia yêu cầu" sửa trực tiếp trong bảng. Đây là quốc gia dùng để **kiểm
 * tra khi khởi chạy** (IP thoát phải khớp). Bấm để mở dropdown; chọn xong lưu ngay.
 * "Không ràng buộc" (rỗng) = không kiểm tra. Giữ state cục bộ để không mất focus
 * khi danh sách re-render (polling).
 */
export function CountryCell({
  value,
  onSave,
}: {
  value: string | null;
  onSave: (country: string | null) => void;
}) {
  const [editing, setEditing] = useState(false);
  const ref = useRef<HTMLSelectElement>(null);

  useEffect(() => {
    if (editing) ref.current?.focus();
  }, [editing]);

  const commit = (next: string) => {
    setEditing(false);
    const normalized = next || null;
    if (normalized !== value) onSave(normalized);
  };

  if (editing) {
    return (
      <select
        ref={ref}
        defaultValue={value ?? ''}
        onChange={(e) => commit(e.target.value)}
        onBlur={() => setEditing(false)}
        className="h-8 w-full rounded border border-primary bg-surface-2 px-2 text-sm outline-none"
      >
        <option value="">Không ràng buộc</option>
        {COUNTRY_CODES.map((code) => (
          <option key={code} value={code}>
            {countryLabel(code)}
          </option>
        ))}
      </select>
    );
  }

  return (
    <button
      onClick={() => setEditing(true)}
      title={value ? 'Bấm để đổi quốc gia yêu cầu' : 'Bấm để đặt quốc gia yêu cầu (gate khi chạy)'}
      className={cn(
        'w-full truncate rounded px-2 py-1 text-left text-sm transition-colors hover:bg-surface-2',
        value ? 'text-fg' : 'text-fg-muted',
      )}
    >
      {value ? countryLabel(value) : '—'}
    </button>
  );
}
