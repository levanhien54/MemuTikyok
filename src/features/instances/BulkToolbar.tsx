import { Play, Square, RotateCw, X } from 'lucide-react';
import { AnimatePresence, motion } from 'framer-motion';
import { Button } from '@/components/ui/Button';

export function BulkToolbar({
  count,
  onStart,
  onStop,
  onReboot,
  onClear,
}: {
  count: number;
  onStart: () => void;
  onStop: () => void;
  onReboot: () => void;
  onClear: () => void;
}) {
  return (
    <AnimatePresence>
      {count > 0 && (
        <motion.div
          initial={{ y: 12, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: 12, opacity: 0 }}
          className="absolute bottom-6 left-1/2 z-20 flex -translate-x-1/2 items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2 shadow-soft"
        >
          <span className="px-2 text-sm font-medium">Đã chọn {count}</span>
          <div className="h-5 w-px bg-border" />
          <Button size="sm" variant="ghost" onClick={onStart}>
            <Play size={15} /> Start
          </Button>
          <Button size="sm" variant="ghost" onClick={onStop}>
            <Square size={15} /> Stop
          </Button>
          <Button size="sm" variant="ghost" onClick={onReboot}>
            <RotateCw size={15} /> Reboot
          </Button>
          <div className="h-5 w-px bg-border" />
          <Button size="icon" variant="ghost" onClick={onClear} aria-label="Bỏ chọn">
            <X size={16} />
          </Button>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
