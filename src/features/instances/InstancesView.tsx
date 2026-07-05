import { useMemo, useState } from 'react';
import { MonitorSmartphone, Plus, RefreshCw, ServerCrash } from 'lucide-react';
import { useInstanceStore } from '@/store/useInstanceStore';
import { toast } from '@/store/useToastStore';
import { formatBytes } from '@/lib/format';
import type { BulkOperation, Instance, HardwareProfile } from '@/types/instance';
import { Button } from '@/components/ui/Button';
import { StateView, LoadingView } from '@/components/ui/StateView';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { InstanceRow } from './InstanceRow';
import { BulkToolbar } from './BulkToolbar';
import { CreateInstanceDialog } from './CreateInstanceDialog';
import { FingerprintDialog } from './FingerprintDialog';

/**
 * Khóa định danh tài khoản để backup/restore session — dùng tên tài khoản TikTok
 * (ỔN ĐỊNH), KHÔNG dùng title VM memuc (tạm thời, có thể tái dùng khi hủy/tạo lại
 * VM → nhiễm chéo phiên). Fallback về title nếu VM chưa gắn tài khoản.
 */
function accountKeyOf(i: Instance): string {
  return i.account?.tiktokUsername?.trim() || i.title;
}

export function InstancesView() {
  const {
    instances,
    selected,
    loadState,
    error,
    search,
    toggleSelect,
    selectAll,
    clearSelection,
    launch,
    stop,
    reboot,
    remove,
    create,
    bulk,
    refresh,
    updateNote,
    updateCountry,
    updateAccount,
    backup,
    restore,
    getHardware,
    installTiktok,
    scanEmulator,
    runWatchSession,
  } = useInstanceStore();

  const [pendingDelete, setPendingDelete] = useState<Instance | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [editInstance, setEditInstance] = useState<Instance | null>(null);
  const [fpState, setFpState] = useState<{
    name: string;
    hardware: HardwareProfile | null | undefined;
  } | null>(null);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return instances;
    return instances.filter(
      (i) =>
        i.title.toLowerCase().includes(q) ||
        (i.account?.tiktokUsername.toLowerCase().includes(q) ?? false),
    );
  }, [instances, search]);

  const allSelected = filtered.length > 0 && filtered.every((i) => selected.has(i.index));

  const doBulk = (op: BulkOperation) => void bulk(op);

  const doLaunch = (instance: Instance) => {
    const name = instance.account?.tiktokUsername || instance.title;
    toast.info(`Đang nạp & chạy "${name}"…`);
    void launch(instance.index, accountKeyOf(instance))
      .then((restored) =>
        toast.success(
          restored
            ? `Đã nạp session & chạy "${name}"`
            : `Đã chạy "${name}" (chưa có phiên lưu — sẽ tạo khi backup)`,
        ),
      )
      .catch((e: unknown) => toast.error(`Chạy lỗi: ${e instanceof Error ? e.message : e}`));
  };

  const doViewFingerprint = (instance: Instance) => {
    const name = instance.account?.tiktokUsername || instance.title;
    // Mở dialog ngay ở trạng thái "đang tải", rồi nạp đúng dữ liệu fingerprint đã lưu.
    setFpState({ name, hardware: undefined });
    void getHardware(instance.index)
      .then((hw) => setFpState({ name, hardware: hw }))
      .catch((e: unknown) => {
        setFpState(null);
        toast.error(`Lỗi nạp fingerprint: ${e instanceof Error ? e.message : e}`);
      });
  };

  const doScanEmulator = (instance: Instance) => {
    const name = instance.account?.tiktokUsername || instance.title;
    toast.info(`Đang scan dấu vết ảo "${name}"…`);
    void scanEmulator(instance.index)
      .then((tells) => {
        const bad = tells.filter((t) => t.detected);
        if (bad.length === 0) {
          toast.success(`"${name}": sạch dấu vết ảo (${tells.length} mục ✓)`);
        } else {
          toast.error(`"${name}": lộ ${bad.length}/${tells.length} — ${bad.map((t) => t.check).join(', ')}`);
        }
      })
      .catch((e: unknown) => toast.error(`Scan lỗi: ${e instanceof Error ? e.message : e}`));
  };

  const doRunSession = (instance: Instance) => {
    const name = instance.account?.tiktokUsername || instance.title;
    toast.info(`Bắt đầu phiên xem giả người "${name}"… (chạy nền)`);
    void runWatchSession(instance.index).catch((e: unknown) =>
      toast.error(`Không bắt đầu được phiên: ${e instanceof Error ? e.message : e}`),
    );
  };

  const doInstallTiktok = (instance: Instance) => {
    const name = instance.account?.tiktokUsername || instance.title;
    toast.info(`Đang cài TikTok vào "${name}"…`);
    void installTiktok(instance.index)
      .then(() => toast.success(`Đã cài TikTok vào "${name}"`))
      .catch((e: unknown) => toast.error(`Cài TikTok lỗi: ${e instanceof Error ? e.message : e}`));
  };

  const doBackup = (instance: Instance) => {
    void backup(instance.index, accountKeyOf(instance))
      .then((rec) =>
        toast.success(`Đã backup "${instance.title}" — nén + mã hóa (${formatBytes(rec.sizeBytes)})`),
      )
      .catch((e: unknown) => toast.error(`Backup lỗi: ${e instanceof Error ? e.message : e}`));
  };

  const doRestore = (instance: Instance) => {
    void restore(instance.index, accountKeyOf(instance))
      .then((rec) => {
        const when = new Date(rec.createdAt).toLocaleString('vi-VN');
        toast.success(`Đã nạp snapshot "${instance.title}" (lúc ${when})`);
      })
      .catch((e: unknown) => toast.error(`Restore lỗi: ${e instanceof Error ? e.message : e}`));
  };

  return (
    <div className="relative flex flex-1 flex-col overflow-hidden">
      {/* Thanh hành động */}
      <div className="flex items-center gap-3 px-6 py-4">
        <h1 className="text-lg font-semibold">Instances</h1>
        <span className="rounded-full bg-surface-2 px-2 py-0.5 text-xs text-fg-muted">
          {instances.length}
        </span>
        <div className="ml-auto flex items-center gap-2">
          <Button size="sm" variant="ghost" onClick={() => void refresh()}>
            <RefreshCw size={15} /> Làm mới
          </Button>
          <Button size="sm" variant="primary" onClick={() => setCreateOpen(true)}>
            <Plus size={15} /> Tạo VM
          </Button>
        </div>
      </div>

      {/* Nội dung theo trạng thái (§10.3) */}
      {loadState === 'loading' && <LoadingView />}

      {loadState === 'error' && (
        <StateView
          icon={<ServerCrash size={32} />}
          title="Không kết nối được với MEmu"
          description={error ?? 'memuc không phản hồi. Kiểm tra đường dẫn cài đặt MEmu trong Settings.'}
          action={
            <Button variant="secondary" onClick={() => void refresh()}>
              Thử lại
            </Button>
          }
        />
      )}

      {loadState === 'ready' && instances.length === 0 && (
        <StateView
          icon={<MonitorSmartphone size={32} />}
          title="Chưa có máy ảo nào"
          description="Tạo máy ảo đầu tiên để bắt đầu quản lý fleet của bạn."
          action={
            <Button variant="primary" onClick={() => setCreateOpen(true)}>
              <Plus size={15} /> Tạo VM
            </Button>
          }
        />
      )}

      {loadState === 'ready' && instances.length > 0 && (
        <div className="flex flex-1 flex-col overflow-hidden px-6 pb-6">
          {/* Header cột */}
          <div className="grid grid-cols-[auto_minmax(11rem,1.5fr)_7rem_7rem_7rem_minmax(8rem,1fr)_auto] items-center gap-3 border-b border-border px-3 pb-2 text-xs uppercase tracking-wide text-fg-muted">
            <input
              type="checkbox"
              checked={allSelected}
              onChange={() => (allSelected ? clearSelection() : selectAll())}
              aria-label="Chọn tất cả"
              className="h-4 w-4 accent-[hsl(var(--primary))]"
            />
            <span>Tài khoản TikTok</span>
            <span>Trạng thái</span>
            <span>Quốc gia</span>
            <span>Khởi chạy</span>
            <span>Ghi chú</span>
            <span className="text-right">Thao tác</span>
          </div>

          {/* Danh sách. Ở quy mô lớn thay bằng @tanstack/react-virtual (§8.6). */}
          <div className="flex flex-1 flex-col gap-1 overflow-auto pt-1">
            {filtered.length === 0 ? (
              <StateView title="Không có kết quả" description={`Không tìm thấy VM khớp "${search}".`} />
            ) : (
              filtered.map((instance) => (
                <InstanceRow
                  key={instance.index}
                  instance={instance}
                  selected={selected.has(instance.index)}
                  onToggleSelect={toggleSelect}
                  onLaunch={() => doLaunch(instance)}
                  onStop={(i) => void stop(i)}
                  onReboot={(i) => void reboot(i)}
                  onRemove={setPendingDelete}
                  onUpdateNote={(i, note) => void updateNote(i, note)}
                  onUpdateCountry={(i, country) => void updateCountry(i, country)}
                  onEdit={(inst) => setEditInstance(inst)}
                  onRunSession={(inst) => doRunSession(inst)}
                  onBackup={doBackup}
                  onRestore={doRestore}
                  onViewFingerprint={doViewFingerprint}
                  onInstallTiktok={doInstallTiktok}
                  onScanEmulator={doScanEmulator}
                />
              ))
            )}
          </div>
        </div>
      )}

      <BulkToolbar
        count={selected.size}
        onStart={() => doBulk('start')}
        onStop={() => doBulk('stop')}
        onReboot={() => doBulk('reboot')}
        onClear={clearSelection}
      />

      <CreateInstanceDialog
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onSubmit={(account, note, country) => {
          void create({ account, note, country });
          setCreateOpen(false);
        }}
      />

      <CreateInstanceDialog
        open={editInstance !== null}
        mode="edit"
        initial={
          editInstance
            ? {
                account: editInstance.account ?? {
                  tiktokUsername: editInstance.title,
                  tiktokPassword: '',
                  twoFa: '',
                  tiktokPasskey: '',
                  email: '',
                  emailPassword: '',
                },
                note: editInstance.note,
                country: editInstance.country,
              }
            : null
        }
        onCancel={() => setEditInstance(null)}
        onSubmit={(account, note, country) => {
          if (editInstance) {
            const i = editInstance.index;
            void updateAccount(i, account);
            void updateNote(i, note);
            void updateCountry(i, country);
            toast.success(`Đã lưu thông tin "${account.tiktokUsername}"`);
          }
          setEditInstance(null);
        }}
      />

      <FingerprintDialog
        open={fpState !== null}
        accountName={fpState?.name ?? ''}
        hardware={fpState?.hardware}
        onClose={() => setFpState(null)}
      />

      <ConfirmDialog
        open={pendingDelete !== null}
        title={`Xóa "${pendingDelete?.title}"?`}
        description="Hành động này không thể hoàn tác. Toàn bộ dữ liệu của máy ảo sẽ bị xóa vĩnh viễn."
        confirmLabel="Xóa vĩnh viễn"
        danger
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete) void remove(pendingDelete.index);
          setPendingDelete(null);
        }}
      />
    </div>
  );
}
