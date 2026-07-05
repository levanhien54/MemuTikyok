import { memo } from 'react';
import {
  Play,
  Square,
  RotateCw,
  Trash2,
  Archive,
  DownloadCloud,
  Fingerprint,
  Download,
  ShieldCheck,
  Pencil,
  Bot,
} from 'lucide-react';
import type { Instance } from '@/types/instance';
import { StatusBadge } from '@/components/ui/StatusBadge';
import { Button } from '@/components/ui/Button';
import { DropdownMenu, type MenuItem } from '@/components/ui/DropdownMenu';
import { formatRelativeTime } from '@/lib/format';
import { cn } from '@/lib/cn';
import { NoteCell } from './NoteCell';
import { CountryCell } from './CountryCell';

interface Props {
  instance: Instance;
  selected: boolean;
  onToggleSelect: (index: number) => void;
  onLaunch: (index: number) => void;
  onStop: (index: number) => void;
  onReboot: (index: number) => void;
  onBackup: (instance: Instance) => void;
  onRestore: (instance: Instance) => void;
  onRemove: (instance: Instance) => void;
  onUpdateNote: (index: number, note: string) => void;
  onUpdateCountry: (index: number, country: string | null) => void;
  onEdit: (instance: Instance) => void;
  onRunSession: (instance: Instance) => void;
  onViewFingerprint: (instance: Instance) => void;
  onInstallTiktok: (instance: Instance) => void;
  onScanEmulator: (instance: Instance) => void;
}

/**
 * Một hàng = một tài khoản TikTok. Nút "Chạy" nạp session & chạy một chạm;
 * các thao tác phụ nằm trong menu ⋮. memo hóa để không render lại cả bảng khi
 * polling cập nhật 1 VM (NFR-P3).
 */
function InstanceRowImpl({
  instance,
  selected,
  onToggleSelect,
  onLaunch,
  onStop,
  onReboot,
  onBackup,
  onRestore,
  onRemove,
  onUpdateNote,
  onUpdateCountry,
  onEdit,
  onRunSession,
  onViewFingerprint,
  onInstallTiktok,
  onScanEmulator,
}: Props) {
  const isRunning = instance.status === 'running';
  const isPending = instance.status === 'starting' || instance.status === 'stopping';
  const username = instance.account?.tiktokUsername || instance.title;
  const email = instance.account?.email;

  const menuItems: MenuItem[] = [];
  menuItems.push({
    label: 'Chỉnh sửa thông tin',
    icon: <Pencil size={15} />,
    onClick: () => onEdit(instance),
  });
  if (isRunning) {
    menuItems.push({
      label: 'Khởi động lại',
      icon: <RotateCw size={15} />,
      onClick: () => onReboot(instance.index),
    });
  }
  menuItems.push({
    label: 'Xem fingerprint',
    icon: <Fingerprint size={15} />,
    onClick: () => onViewFingerprint(instance),
  });
  menuItems.push({
    label: 'Kiểm tra dấu vết ảo',
    icon: <ShieldCheck size={15} />,
    onClick: () => onScanEmulator(instance),
    disabled: !isRunning,
  });
  menuItems.push({
    label: 'Cài TikTok',
    icon: <Download size={15} />,
    onClick: () => onInstallTiktok(instance),
    disabled: isPending,
  });
  menuItems.push({
    label: 'Chạy phiên xem (giả người)',
    icon: <Bot size={15} />,
    onClick: () => onRunSession(instance),
    disabled: !isRunning,
  });
  menuItems.push({
    label: 'Backup dữ liệu',
    icon: <Archive size={15} />,
    onClick: () => onBackup(instance),
    disabled: isPending,
  });
  menuItems.push({
    label: 'Restore dữ liệu',
    icon: <DownloadCloud size={15} />,
    onClick: () => onRestore(instance),
    disabled: isPending,
  });
  menuItems.push({
    label: 'Xóa máy ảo',
    icon: <Trash2 size={15} />,
    onClick: () => onRemove(instance),
    danger: true,
    disabled: isPending,
  });

  return (
    <div
      className={cn(
        'grid grid-cols-[auto_minmax(11rem,1.5fr)_7rem_7rem_7rem_minmax(8rem,1fr)_auto] items-center gap-3 rounded-md border border-transparent px-3 py-2.5 transition-colors hover:bg-surface-2',
        selected && 'border-primary/40 bg-primary/5',
      )}
    >
      <input
        type="checkbox"
        checked={selected}
        onChange={() => onToggleSelect(instance.index)}
        aria-label={`Chọn ${username}`}
        className="h-4 w-4 accent-[hsl(var(--primary))]"
      />

      {/* Thông tin tài khoản TikTok */}
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium">{username}</span>
          <span className="shrink-0 font-mono text-[11px] text-fg-muted">#{instance.index}</span>
        </div>
        <div className="truncate text-xs text-fg-muted">{email || 'chưa có email'}</div>
      </div>

      <StatusBadge status={instance.status} />
      <CountryCell
        value={instance.country}
        onSave={(country) => onUpdateCountry(instance.index, country)}
      />
      <span
        className="truncate text-sm text-fg-muted"
        title={instance.lastLaunchedAt ? new Date(instance.lastLaunchedAt).toLocaleString('vi-VN') : ''}
      >
        {formatRelativeTime(instance.lastLaunchedAt)}
      </span>
      <NoteCell value={instance.note} onSave={(note) => onUpdateNote(instance.index, note)} />

      {/* Hành động: Chạy (một chạm) + menu phụ */}
      <div className="flex items-center justify-end gap-1">
        {isRunning ? (
          <Button
            size="icon"
            variant="ghost"
            disabled={isPending}
            onClick={() => onStop(instance.index)}
            aria-label="Dừng"
            title="Dừng máy ảo"
            className="text-danger hover:bg-danger/10"
          >
            <Square size={16} />
          </Button>
        ) : (
          <Button
            size="icon"
            variant="primary"
            disabled={isPending}
            onClick={() => onLaunch(instance.index)}
            aria-label="Chạy"
            title="Nạp tài khoản & chạy"
          >
            <Play size={16} />
          </Button>
        )}
        <DropdownMenu items={menuItems} />
      </div>
    </div>
  );
}

export const InstanceRow = memo(InstanceRowImpl);
