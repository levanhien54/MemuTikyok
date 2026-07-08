import { LayoutDashboard, MonitorSmartphone, ScrollText, Settings, Zap } from 'lucide-react';
import { cn } from '@/lib/cn';

export type View = 'dashboard' | 'instances' | 'automation' | 'logs' | 'settings';

const NAV: { id: View; label: string; icon: typeof LayoutDashboard; soon?: boolean }[] = [
  { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard },
  { id: 'instances', label: 'Tài khoản', icon: MonitorSmartphone },
  { id: 'automation', label: 'Automation', icon: Zap, soon: true },
  { id: 'logs', label: 'Logs', icon: ScrollText },
  { id: 'settings', label: 'Settings', icon: Settings },
];

/**
 * Sidebar thu gọn dạng thanh icon (rail 68px), tự MỞ RỘNG khi hover (240px).
 * Dùng vị trí absolute (overlay) để khi mở rộng không đẩy nội dung — tránh
 * layout nhảy/chồng lặp. Nhãn hiện dần bằng opacity theo `group-hover`.
 */
export function Sidebar({ current, onNavigate }: { current: View; onNavigate: (v: View) => void }) {
  return (
    <aside className="group absolute inset-y-0 left-0 z-40 flex w-[68px] flex-col overflow-hidden border-r border-border bg-surface/95 backdrop-blur-sm transition-[width] duration-200 ease-out hover:w-60 hover:shadow-soft">
      {/* Logo */}
      <div className="flex h-16 shrink-0 items-center gap-3 px-[15px]">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-gradient-primary text-white shadow-glow">
          <MonitorSmartphone size={18} />
        </div>
        <div className="min-w-0 opacity-0 transition-opacity duration-200 group-hover:opacity-100">
          <div className="whitespace-nowrap text-sm font-semibold leading-tight">MuMu Manager</div>
          <div className="whitespace-nowrap text-xs text-fg-muted">v0.1.0</div>
        </div>
      </div>

      {/* Điều hướng */}
      <nav className="flex flex-col gap-1 px-3 pt-2">
        {NAV.map(({ id, label, icon: Icon, soon }) => (
          <button
            key={id}
            disabled={soon}
            onClick={() => onNavigate(id)}
            title={label}
            className={cn(
              'flex items-center gap-3 rounded-md py-2 pl-[9px] pr-3 text-sm transition-colors',
              current === id
                ? 'bg-surface-2 font-medium text-fg'
                : 'text-fg-muted hover:bg-surface-2 hover:text-fg',
              soon && 'cursor-not-allowed opacity-50 hover:bg-transparent',
            )}
          >
            <Icon size={18} className="shrink-0" />
            <span className="whitespace-nowrap opacity-0 transition-opacity duration-200 group-hover:opacity-100">
              {label}
            </span>
            {soon && (
              <span className="ml-auto whitespace-nowrap rounded bg-muted px-1.5 py-0.5 text-[10px] uppercase opacity-0 transition-opacity group-hover:opacity-100">
                soon
              </span>
            )}
          </button>
        ))}
      </nav>
    </aside>
  );
}
