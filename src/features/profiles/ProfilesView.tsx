import { useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { MonitorSmartphone, Plus, RefreshCw, ServerCrash } from 'lucide-react';
import { useProfileStore } from '@/store/useProfileStore';
import { getBackend } from '@/lib';
import { toast } from '@/store/useToastStore';
import type { AccountProfile, ProfileView } from '@/types/instance';
import { Button } from '@/components/ui/Button';
import { StateView, LoadingView } from '@/components/ui/StateView';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { CreateInstanceDialog } from '@/features/instances/CreateInstanceDialog';
import { FingerprintDialog } from '@/features/instances/FingerprintDialog';
import { ProfileRow } from './ProfileRow';

export function ProfilesView() {
  const { profiles, loadState, error, search, refresh, create, update, run, stop, remove } =
    useProfileStore();

  const [createOpen, setCreateOpen] = useState(false);
  const [editProfile, setEditProfile] = useState<ProfileView | null>(null);
  const [fpState, setFpState] = useState<{ name: string; view: ProfileView } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<ProfileView | null>(null);
  const [uploadingSet, setUploadingSet] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<Set<string>>(new Set());

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return profiles;
    return profiles.filter((v) => v.profile.username.toLowerCase().includes(q));
  }, [profiles, search]);

  const withBusy = async (username: string, fn: () => Promise<void>) => {
    setBusy((b) => new Set(b).add(username));
    try {
      await fn();
    } finally {
      setBusy((b) => {
        const n = new Set(b);
        n.delete(username);
        return n;
      });
    }
  };

  const doRun = (username: string) => {
    toast.info(`Đang cấp VM & chạy "${username}"…`);
    void withBusy(username, () =>
      run(username)
        .then((vm) => toast.success(`"${username}" đang chạy trên VM #${vm}`))
        .catch((e: unknown) => toast.error(`Chạy lỗi: ${e instanceof Error ? e.message : e}`)),
    );
  };

  const doStop = (username: string) => {
    toast.info(`Đang backup & dừng "${username}"…`);
    void withBusy(username, () =>
      stop(username)
        .then(() => toast.success(`Đã backup & dừng "${username}" (đã hủy VM)`))
        .catch((e: unknown) => toast.error(`Dừng lỗi: ${e instanceof Error ? e.message : e}`)),
    );
  };

  const doScanEmulator = (view: ProfileView) => {
    if (view.runningVm == null) return;
    toast.info(`Đang scan dấu vết ảo "${view.profile.username}"…`);
    void getBackend()
      .scanEmulator(view.runningVm)
      .then((tells) => {
        const bad = tells.filter((t) => t.detected);
        if (bad.length === 0) toast.success(`"${view.profile.username}": sạch dấu vết ảo`);
        else
          toast.error(
            `"${view.profile.username}": lộ ${bad.length}/${tells.length} — ${bad
              .map((t) => t.check)
              .join(', ')}`,
          );
      })
      .catch((e: unknown) => toast.error(`Scan lỗi: ${e instanceof Error ? e.message : e}`));
  };

  const doRunSession = (view: ProfileView) => {
    if (view.runningVm == null) return;
    toast.info(`Bắt đầu phiên xem giả người "${view.profile.username}"… (chạy nền)`);
    void getBackend()
      .runWatchSession(view.runningVm)
      .catch((e: unknown) =>
        toast.error(`Không bắt đầu được phiên: ${e instanceof Error ? e.message : e}`),
      );
  };

  const doUploadVideo = async (view: ProfileView) => {
    if (view.runningVm == null) return;
    try {
      let selected: string | string[] | null = null;
      if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
        selected = await open({
          title: 'Chọn file video (MP4, MOV, v.v.)',
          filters: [{ name: 'Video', extensions: ['mp4', 'mov', 'avi', 'mkv', 'webm'] }],
          multiple: false,
        });
      } else {
        selected = 'C:\\mock\\test_video.mp4';
      }
      if (selected === null || typeof selected !== 'string') return;

      setUploadingSet((prev) => new Set(prev).add(view.profile.username));
      toast.info(`Đang đưa video vào máy ảo "${view.profile.username}"…`);
      await getBackend().uploadVideoToVm(view.runningVm, selected);
      toast.success(`Đã đưa video vào VM thành công! Bạn có thể mở ứng dụng TikTok để đăng.`);
    } catch (e: unknown) {
      toast.error(`Lỗi chuyển video: ${e instanceof Error ? e.message : e}`);
    } finally {
      setUploadingSet((prev) => {
        const next = new Set(prev);
        next.delete(view.profile.username);
        return next;
      });
    }
  };

  return (
    <div className="flex flex-1 flex-col overflow-hidden p-6">
      <div className="mb-4 flex items-center gap-3">
        <div className="flex items-center gap-2">
          <h1 className="text-xl font-semibold">Tài khoản (Profile)</h1>
          <span className="rounded-full bg-surface-2 px-2 py-0.5 text-xs text-fg-muted">
            {profiles.length}
          </span>
        </div>
        <Button variant="ghost" className="ml-auto" onClick={() => void refresh()}>
          <RefreshCw size={15} /> Làm mới
        </Button>
        <Button variant="primary" onClick={() => setCreateOpen(true)}>
          <Plus size={15} /> Tạo profile
        </Button>
      </div>

      {loadState === 'loading' && <LoadingView />}
      {loadState === 'error' && (
        <StateView
          icon={<ServerCrash size={40} />}
          title="Không tải được danh sách"
          description={error ?? 'Lỗi backend.'}
        />
      )}
      {loadState === 'ready' && filtered.length === 0 && (
        <StateView
          icon={<MonitorSmartphone size={40} />}
          title={search ? 'Không có kết quả' : 'Chưa có profile nào'}
          description={
            search ? 'Thử từ khóa khác.' : 'Bấm "Tạo profile" để thêm tài khoản (không tạo VM).'
          }
        />
      )}

      {loadState === 'ready' && filtered.length > 0 && (
        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="grid grid-cols-[minmax(11rem,1.5fr)_8rem_7rem_minmax(8rem,1fr)_7rem_auto] gap-3 border-b border-border px-3 pb-2 text-xs font-medium uppercase text-fg-muted">
            <span>Tài khoản TikTok</span>
            <span>Trạng thái</span>
            <span>Quốc gia</span>
            <span>Ghi chú</span>
            <span>Chạy gần</span>
            <span className="text-right">Thao tác</span>
          </div>
          <div className="flex-1 overflow-y-auto pt-1">
            {filtered.map((view) => (
              <ProfileRow
                key={view.profile.username}
                view={view}
                busy={busy.has(view.profile.username)}
                isUploading={uploadingSet.has(view.profile.username)}
                onRun={doRun}
                onStop={doStop}
                onEdit={(v) => setEditProfile(v)}
                onViewFingerprint={(v) => setFpState({ name: v.profile.username, view: v })}
                onScanEmulator={doScanEmulator}
                onRunSession={doRunSession}
                onUploadVideo={doUploadVideo}
                onDelete={(v) => setPendingDelete(v)}
                onUpdateNote={(username, note) => {
                  const p = profiles.find((x) => x.profile.username === username)?.profile;
                  if (p) void update(username, p.account, note, p.country);
                }}
                onUpdateCountry={(username, country) => {
                  const p = profiles.find((x) => x.profile.username === username)?.profile;
                  if (p) void update(username, p.account, p.note, country);
                }}
              />
            ))}
          </div>
        </div>
      )}

      {/* Tạo profile */}
      <CreateInstanceDialog
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onSubmit={(account, note, country) => {
          void create(account, note, country)
            .then(() => toast.success(`Đã tạo profile "${account.tiktokUsername}"`))
            .catch((e: unknown) => toast.error(`Tạo lỗi: ${e instanceof Error ? e.message : e}`));
          setCreateOpen(false);
        }}
      />

      {/* Sửa profile */}
      <CreateInstanceDialog
        open={editProfile !== null}
        mode="edit"
        initial={
          editProfile
            ? {
                account: editProfile.profile.account,
                note: editProfile.profile.note,
                country: editProfile.profile.country,
              }
            : null
        }
        onCancel={() => setEditProfile(null)}
        onSubmit={(account: AccountProfile, note, country) => {
          if (editProfile) {
            void update(editProfile.profile.username, account, note, country)
              .then(() => toast.success(`Đã lưu "${editProfile.profile.username}"`))
              .catch((e: unknown) => toast.error(`Lưu lỗi: ${e instanceof Error ? e.message : e}`));
          }
          setEditProfile(null);
        }}
      />

      <FingerprintDialog
        open={fpState !== null}
        accountName={fpState?.name ?? ''}
        hardware={fpState?.view.profile.hardware}
        onClose={() => setFpState(null)}
      />

      <ConfirmDialog
        open={pendingDelete !== null}
        title={`Xóa profile "${pendingDelete?.profile.username}"?`}
        description="Xóa tài khoản khỏi danh sách. Nếu đang chạy sẽ backup + hủy VM trước. Snapshot session vẫn giữ."
        confirmLabel="Xóa profile"
        danger
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete) {
            const u = pendingDelete.profile.username;
            void remove(u)
              .then(() => toast.success(`Đã xóa "${u}"`))
              .catch((e: unknown) => toast.error(`Xóa lỗi: ${e instanceof Error ? e.message : e}`));
          }
          setPendingDelete(null);
        }}
      />
    </div>
  );
}
