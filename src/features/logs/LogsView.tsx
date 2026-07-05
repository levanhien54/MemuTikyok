import { useCallback, useEffect, useRef, useState } from 'react';
import { ScrollText, Pause, Play, ArrowDownToLine } from 'lucide-react';
import { getBackend } from '@/lib';
import { Button } from '@/components/ui/Button';
import { cn } from '@/lib/cn';

type Level = 'ERROR' | 'WARN' | 'INFO' | 'OTHER';

/** Đoán mức log từ dòng text (subscriber ghi "... LEVEL target: message"). */
function levelOf(line: string): Level {
  if (/\bERROR\b/.test(line)) return 'ERROR';
  if (/\bWARN\b/.test(line)) return 'WARN';
  if (/\bINFO\b/.test(line)) return 'INFO';
  return 'OTHER';
}

const LEVEL_STYLE: Record<Level, string> = {
  ERROR: 'text-danger',
  WARN: 'text-warning',
  INFO: 'text-fg-muted',
  OTHER: 'text-fg-muted/70',
};

const FILTERS: { label: string; min: number }[] = [
  { label: 'Tất cả', min: 0 },
  { label: 'Info+', min: 1 },
  { label: 'Warn+', min: 2 },
  { label: 'Error', min: 3 },
];
const RANK: Record<Level, number> = { OTHER: 0, INFO: 1, WARN: 2, ERROR: 3 };

/**
 * Trình xem log ứng dụng (FR-E-4): poll ring buffer backend (`get_logs`) mỗi 2s,
 * tô màu theo mức, lọc, auto-scroll. Dùng để chẩn đoán khi một lần Chạy thất bại
 * (provision lỗi, install retry, reconcile hủy VM mồ côi, backup fail…).
 */
export function LogsView() {
  const [lines, setLines] = useState<string[]>([]);
  const [live, setLive] = useState(true);
  const [minLevel, setMinLevel] = useState(0);
  const boxRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);

  const refresh = useCallback(() => {
    void getBackend()
      .getLogs()
      .then(setLines)
      .catch(() => {
        /* backend chưa sẵn sàng — bỏ qua */
      });
  }, []);

  useEffect(() => {
    refresh();
    if (!live) return;
    const id = setInterval(refresh, 2000);
    return () => clearInterval(id);
  }, [live, refresh]);

  // Auto-scroll xuống đáy nếu người dùng đang ở đáy.
  useEffect(() => {
    const el = boxRef.current;
    if (el && atBottomRef.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const onScroll = () => {
    const el = boxRef.current;
    if (!el) return;
    atBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  const shown = lines.filter((l) => RANK[levelOf(l)] >= minLevel);

  return (
    <div className="flex flex-1 flex-col overflow-hidden p-6">
      <div className="mb-4 flex items-center gap-3">
        <h1 className="text-lg font-semibold">Logs</h1>
        <span className="rounded-full bg-surface-2 px-2 py-0.5 text-xs text-fg-muted">
          {shown.length}/{lines.length}
        </span>
        <div className="ml-auto flex items-center gap-1 rounded-md border border-border bg-surface-2 p-0.5">
          {FILTERS.map((f) => (
            <button
              key={f.label}
              onClick={() => setMinLevel(f.min)}
              className={cn(
                'rounded px-2.5 py-1 text-xs transition-colors',
                minLevel === f.min ? 'bg-primary text-white' : 'text-fg-muted hover:text-fg',
              )}
              title={`Lọc ${f.label}`}
            >
              {f.label}
            </button>
          ))}
        </div>
        <Button variant="ghost" size="sm" onClick={() => setLive((v) => !v)}>
          {live ? <Pause size={15} /> : <Play size={15} />} {live ? 'Tạm dừng' : 'Theo dõi'}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            atBottomRef.current = true;
            refresh();
          }}
        >
          <ArrowDownToLine size={15} /> Cuối
        </Button>
      </div>

      {shown.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center text-fg-muted">
          <ScrollText size={32} />
          <p className="text-sm">Chưa có log phù hợp bộ lọc.</p>
        </div>
      ) : (
        <div
          ref={boxRef}
          onScroll={onScroll}
          className="flex-1 overflow-auto rounded-lg border border-border bg-surface p-3 font-mono text-xs leading-relaxed"
        >
          {shown.map((l, i) => (
            <div key={i} className={cn('whitespace-pre-wrap break-all', LEVEL_STYLE[levelOf(l)])}>
              {l}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
