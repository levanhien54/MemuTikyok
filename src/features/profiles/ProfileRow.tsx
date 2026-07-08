import { memo } from 'react';
import {
  Play,
  Square,
  Fingerprint,
  ShieldCheck,
  Bot,
  Pencil,
  Trash2,
  Video,
  Loader2,
} from 'lucide-react';
import type { ProfileView } from '@/types/instance';
import { Button } from '@/components/ui/Button';
import { DropdownMenu, type MenuItem } from '@/components/ui/DropdownMenu';
import { formatRelativeTime } from '@/lib/format';
import { cn } from '@/lib/cn';
import { NoteCell } from '@/features/instances/NoteCell';
import { CountryCell } from '@/features/instances/CountryCell';

interface Props {
  view: ProfileView;
  busy: boolean;
  isUploading?: boolean;
  onRun: (username: string) => void;
  onStop: (username: string) => void;
  onEdit: (view: ProfileView) => void;
  onViewFingerprint: (view: ProfileView) => void;
  onScanEmulator: (view: ProfileView) => void;
  onRunSession: (view: ProfileView) => void;
  onUploadVideo: (view: ProfileView) => void;
  onDelete: (view: ProfileView) => void;
  onUpdateNote: (username: string, note: string) => void;
  onUpdateCountry: (username: string, country: string | null) => void;
}

/**
 * Một hàng = một PROFILE (tài khoản, dữ liệu bền). "Chạy" cấp VM từ pool; "Dừng"
 * backup + hủy VM (disposable). Trạng thái cho biết profile đang chạy trên VM nào.
 */
function ProfileRowImpl({
  view,
  busy,
  isUploading,
  onRun,
  onStop,
  onEdit,
  onViewFingerprint,
  onScanEmulator,
  onRunSession,
  onUploadVideo,
  onDelete,
  onUpdateNote,
  onUpdateCountry,
}: Props) {
  const { profile } = view;
  const running = view.runningVm != null;
  const username = profile.account.tiktokUsername || profile.username;
  const email = profile.account.email;

  const menuItems: MenuItem[] = [
    { label: 'Chỉnh sửa thông tin', icon: <Pencil size={15} />, onClick: () => onEdit(view) },
    {
      label: 'Xem fingerprint',
      icon: <Fingerprint size={15} />,
      onClick: () => onViewFingerprint(view),
    },
    {
      label: 'Kiểm tra dấu vết ảo',
      icon: <ShieldCheck size={15} />,
      onClick: () => onScanEmulator(view),
      disabled: !running,
    },
    {
      label: 'Chạy phiên xem (giả người)',
      icon: <Bot size={15} />,
      onClick: () => onRunSession(view),
      disabled: !running,
    },
    {
      label: 'Xóa profile',
      icon: <Trash2 size={15} />,
      onClick: () => onDelete(view),
      danger: true,
    },
  ];

  return (
    <div
      className={cn(
        'grid grid-cols-[minmax(11rem,1.5fr)_8rem_7rem_minmax(8rem,1fr)_7rem_auto] items-center gap-3 rounded-md border border-transparent px-3 py-2.5 transition-colors hover:bg-surface-2',
      )}
    >
      {/* Tài khoản */}
      <div className="min-w-0">
        <div className="truncate font-medium">{username}</div>
        <div className="truncate text-xs text-fg-muted">{email || 'chưa có email'}</div>
      </div>

      {/* Trạng thái */}
      <span
        className={cn(
          'inline-flex w-fit items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium',
          running ? 'bg-success/15 text-success' : 'bg-surface-2 text-fg-muted',
        )}
        title={running ? `Đang chạy trên VM #${view.runningVm}` : 'Chưa chạy'}
      >
        <span
          className={cn('h-1.5 w-1.5 rounded-full', running ? 'bg-success' : 'bg-fg-muted/50')}
        />
        {running ? `Chạy · VM #${view.runningVm}` : 'Nghỉ'}
      </span>

      <CountryCell value={profile.country} onSave={(c) => onUpdateCountry(profile.username, c)} />
      <NoteCell value={profile.note} onSave={(n) => onUpdateNote(profile.username, n)} />

      <span
        className="truncate text-sm text-fg-muted"
        title={profile.lastRunAt ? new Date(profile.lastRunAt).toLocaleString('vi-VN') : ''}
      >
        {formatRelativeTime(profile.lastRunAt)}
      </span>

      {/* Hành động */}
      <div className="flex items-center justify-end gap-1">
        {running ? (
          <>
            <Button
              size="icon"
              variant="ghost"
              disabled={busy || isUploading}
              onClick={() => onUploadVideo(view)}
              aria-label="Nạp Video"
              title="Đưa video vào bộ sưu tập máy ảo"
              className="text-primary hover:bg-primary/10"
            >
              {isUploading ? <Loader2 size={16} className="animate-spin" /> : <Video size={16} />}
            </Button>
            <Button
              size="icon"
              variant="ghost"
              disabled={busy}
              onClick={() => onStop(profile.username)}
              aria-label="Dừng"
              title="Dừng + backup phiên (hủy VM)"
              className="text-danger hover:bg-danger/10"
            >
              <Square size={16} />
            </Button>
          </>
        ) : (
          <Button
            size="icon"
            variant="primary"
            disabled={busy}
            onClick={() => onRun(profile.username)}
            aria-label="Chạy"
            title="Cấp VM + nạp session & chạy"
          >
            <Play size={16} />
          </Button>
        )}
        <DropdownMenu items={menuItems} />
      </div>
    </div>
  );
}

export const ProfileRow = memo(ProfileRowImpl);
