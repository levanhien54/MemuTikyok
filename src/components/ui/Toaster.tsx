import { AnimatePresence, motion } from 'framer-motion';
import { CheckCircle2, XCircle, Info, X } from 'lucide-react';
import { useToastStore, type ToastKind } from '@/store/useToastStore';
import { cn } from '@/lib/cn';

const ICON: Record<ToastKind, typeof CheckCircle2> = {
  success: CheckCircle2,
  error: XCircle,
  info: Info,
};

const BORDER_COLOR: Record<ToastKind, string> = {
  success: 'border-success/30 shadow-success/10',
  error: 'border-danger/30 shadow-danger/10',
  info: 'border-primary/30 shadow-primary/10',
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
    <div className="pointer-events-none fixed right-6 top-6 z-[100] flex w-[340px] flex-col gap-3">
      <AnimatePresence>
        {toasts.map((t) => {
          const Icon = ICON[t.kind];
          return (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, y: -20, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, x: 100, scale: 0.95 }}
              transition={{ type: 'spring', bounce: 0.3 }}
              className={cn(
                'pointer-events-auto flex items-start gap-3 overflow-hidden rounded-xl',
                'bg-surface/80 backdrop-blur-xl shadow-2xl border',
                BORDER_COLOR[t.kind],
                'px-4 py-3.5 relative',
              )}
            >
              {/* Nền phản quang mượt mà */}
              <div className="absolute inset-0 bg-gradient-to-br from-white/10 to-transparent pointer-events-none" />

              <Icon size={18} className={cn('mt-0.5 shrink-0 drop-shadow-sm', COLOR[t.kind])} />
              <p className="flex-1 text-sm font-medium text-fg drop-shadow-sm z-10 leading-relaxed">
                {t.message}
              </p>
              <button
                onClick={() => dismiss(t.id)}
                aria-label="Đóng"
                className="z-10 text-fg-muted transition-colors hover:text-fg mt-0.5"
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
