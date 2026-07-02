import { AnimatePresence, motion } from 'framer-motion';
import { CheckCircle2, XCircle, Info, X } from 'lucide-react';
import { useToastStore, type ToastKind } from '@/store/useToastStore';

const ICON: Record<ToastKind, typeof CheckCircle2> = {
  success: CheckCircle2,
  error: XCircle,
  info: Info,
};

const COLOR: Record<ToastKind, string> = {
  success: 'text-success',
  error: 'text-danger',
  info: 'text-primary',
};

export function Toaster() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);

  return (
    <div className="pointer-events-none fixed right-4 top-4 z-[100] flex w-80 flex-col gap-2">
      <AnimatePresence>
        {toasts.map((t) => {
          const Icon = ICON[t.kind];
          return (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, x: 40 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 40 }}
              className="pointer-events-auto flex items-start gap-3 rounded-lg border border-border bg-surface px-4 py-3 shadow-soft"
            >
              <Icon size={18} className={`mt-0.5 shrink-0 ${COLOR[t.kind]}`} />
              <p className="flex-1 text-sm">{t.message}</p>
              <button
                onClick={() => dismiss(t.id)}
                aria-label="Đóng"
                className="text-fg-muted hover:text-fg"
              >
                <X size={15} />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
