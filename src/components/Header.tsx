import { Moon, Search, Sun } from 'lucide-react';
import { Button } from './ui/Button';
import { useProfileStore } from '@/store/useProfileStore';
import { useSettingsStore } from '@/store/useSettingsStore';

export function Header() {
  const search = useProfileStore((s) => s.search);
  const setSearch = useProfileStore((s) => s.setSearch);
  const theme = useSettingsStore((s) => s.settings?.theme ?? 'dark');
  const toggleTheme = useSettingsStore((s) => s.toggleTheme);

  return (
    <header className="flex h-16 shrink-0 items-center gap-4 border-b border-border bg-surface/40 px-6">
      <div className="relative w-full max-w-sm">
        <Search
          size={16}
          className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-fg-muted"
        />
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Tìm tài khoản theo tên…"
          aria-label="Tìm kiếm tài khoản"
          className="h-10 w-full rounded-md border border-border bg-surface pl-9 pr-3 text-sm outline-none transition-colors focus:border-primary"
        />
      </div>

      <div className="ml-auto flex items-center gap-2">
        <Button
          size="icon"
          variant="ghost"
          onClick={() => void toggleTheme()}
          aria-label="Đổi giao diện sáng/tối"
        >
          {theme === 'dark' ? <Sun size={18} /> : <Moon size={18} />}
        </Button>
      </div>
    </header>
  );
}
