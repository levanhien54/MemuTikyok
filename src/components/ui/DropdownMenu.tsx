import { useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { MoreVertical } from 'lucide-react';
import { AnimatePresence, motion } from 'framer-motion';
import { Button } from './Button';
import { cn } from '@/lib/cn';

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
}

const MENU_WIDTH = 190;

/**
 * Menu phụ (⋮). Render qua portal + vị trí fixed để không bị cắt bởi container
 * có overflow. Đóng khi bấm ra ngoài hoặc nhấn Escape.
 */
export function DropdownMenu({
  items,
  label = 'Thêm thao tác',
}: {
  items: MenuItem[];
  label?: string;
}) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState({ top: 0, left: 0 });
  const btnRef = useRef<HTMLButtonElement>(null);

  const toggle = () => {
    if (open) {
      setOpen(false);
      return;
    }
    const r = btnRef.current?.getBoundingClientRect();
    if (r) setPos({ top: r.bottom + 6, left: Math.max(8, r.right - MENU_WIDTH) });
    setOpen(true);
  };

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!btnRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    const onScroll = () => setOpen(false);
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [open]);

  return (
    <>
      <Button ref={btnRef} size="icon" variant="ghost" onClick={toggle} aria-label={label}>
        <MoreVertical size={16} />
      </Button>
      {createPortal(
        <AnimatePresence>
          {open && (
            <motion.div
              initial={{ opacity: 0, scale: 0.96, y: -4 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.96 }}
              transition={{ duration: 0.12 }}
              role="menu"
              style={{ position: 'fixed', top: pos.top, left: pos.left, width: MENU_WIDTH }}
              className="z-[90] overflow-hidden rounded-lg border border-border bg-surface p-1 shadow-soft"
            >
              {items.map((item, i) => (
                <button
                  key={i}
                  role="menuitem"
                  disabled={item.disabled}
                  onClick={() => {
                    setOpen(false);
                    item.onClick();
                  }}
                  className={cn(
                    'flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-left text-sm transition-colors',
                    'disabled:opacity-40 disabled:pointer-events-none',
                    item.danger ? 'text-danger hover:bg-danger/10' : 'text-fg hover:bg-surface-2',
                  )}
                >
                  {item.icon && <span className="shrink-0">{item.icon}</span>}
                  {item.label}
                </button>
              ))}
            </motion.div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </>
  );
}
