import { useEffect, useRef, useState } from 'react';
import { cn } from '@/lib/cn';

/**
 * Ô ghi chú sửa trực tiếp trong bảng. Lưu khi blur hoặc Enter; Esc để hủy.
 * Giữ state cục bộ để không mất focus khi danh sách re-render (polling).
 */
export function NoteCell({ value, onSave }: { value: string; onSave: (note: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) ref.current?.focus();
  }, [editing]);

  // Đồng bộ khi giá trị ngoài đổi và không đang sửa.
  useEffect(() => {
    if (!editing) setDraft(value);
  }, [value, editing]);

  const commit = () => {
    setEditing(false);
    const next = draft.trim();
    if (next !== value) onSave(next);
  };

  if (editing) {
    return (
      <input
        ref={ref}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
          if (e.key === 'Escape') {
            setDraft(value);
            setEditing(false);
          }
        }}
        placeholder="Nhập ghi chú…"
        className="h-8 w-full rounded border border-primary bg-surface-2 px-2 text-sm outline-none"
      />
    );
  }

  return (
    <button
      onClick={() => setEditing(true)}
      title="Bấm để sửa ghi chú"
      className={cn(
        'w-full truncate rounded px-2 py-1 text-left text-sm transition-colors hover:bg-surface-2',
        value ? 'text-fg' : 'text-fg-muted italic',
      )}
    >
      {value || 'Thêm ghi chú…'}
    </button>
  );
}
