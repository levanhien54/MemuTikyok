import { AnimatePresence, motion } from 'framer-motion';
import { useRef } from 'react';
import { Button } from './Button';
import { useModalFocusTrap } from './useModalFocusTrap';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  description: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Hộp thoại xác nhận cho thao tác nguy hiểm (§10.3 SRS — vd xóa VM).
 * Bắt buộc dùng cho FR-B-3 (xác nhận 2 bước).
 */
export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel = 'Xác nhận',
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  useModalFocusTrap(open, dialogRef, onCancel);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={onCancel}
        >
          <motion.div
            ref={dialogRef}
            role="dialog"
            aria-modal="true"
            tabIndex={-1}
            className="w-full max-w-md rounded-lg border border-border bg-surface p-6 shadow-soft"
            initial={{ scale: 0.95, y: 8 }}
            animate={{ scale: 1, y: 0 }}
            exit={{ scale: 0.95, opacity: 0 }}
            onClick={(e) => e.stopPropagation()}
          >
            <h2 className="text-lg font-semibold">{title}</h2>
            <p className="mt-2 text-sm text-fg-muted">{description}</p>
            <div className="mt-6 flex justify-end gap-3">
              <Button variant="ghost" onClick={onCancel}>
                Hủy
              </Button>
              <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm}>
                {confirmLabel}
              </Button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
