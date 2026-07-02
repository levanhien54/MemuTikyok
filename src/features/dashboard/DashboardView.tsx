import { useEffect, useMemo, useState } from 'react';
import { Activity, MonitorSmartphone, Play, HardDrive, Flame, RefreshCw } from 'lucide-react';
import { useInstanceStore } from '@/store/useInstanceStore';
import { useSettingsStore } from '@/store/useSettingsStore';
import { getBackend } from '@/lib';
import { toast } from '@/store/useToastStore';
import { formatBytes } from '@/lib/format';
import { Button } from '@/components/ui/Button';

function StatCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="rounded-lg border border-border bg-surface p-5 shadow-soft">
      <div className="flex items-center justify-between">
        <span className="text-sm text-fg-muted">{label}</span>
        <span className="text-primary">{icon}</span>
      </div>
      <div className="mt-3 text-2xl font-semibold">{value}</div>
    </div>
  );
}

export function DashboardView() {
  const instances = useInstanceStore((s) => s.instances);
  const settings = useSettingsStore((s) => s.settings);
  const [poolSize, setPoolSize] = useState(0);
  const [refilling, setRefilling] = useState(false);

  const stats = useMemo(() => {
    const running = instances.filter((i) => i.status === 'running').length;
    const disk = instances.reduce((sum, i) => sum + (i.diskUsageBytes ?? 0), 0);
    return { total: instances.length, running, disk };
  }, [instances]);

  useEffect(() => {
    void getBackend().getPoolSize().then(setPoolSize);
  }, []);

  const target = settings?.warmPoolTarget ?? 0;
  const base = settings?.poolBaseIndex;

  const refill = () => {
    const t = target > 0 ? target : 3;
    const b = base ?? instances[0]?.index ?? 0;
    setRefilling(true);
    void getBackend()
      .refillPool(b, t)
      .then((n) => {
        setPoolSize(n);
        toast.success(`Warm pool: ${n} VM nóng sẵn sàng (clone từ VM #${b})`);
      })
      .catch((e: unknown) => toast.error(`Nạp pool lỗi: ${e instanceof Error ? e.message : e}`))
      .finally(() => setRefilling(false));
  };

  return (
    <div className="flex-1 overflow-auto p-6">
      <h1 className="mb-6 text-lg font-semibold">Dashboard</h1>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard icon={<MonitorSmartphone size={18} />} label="Tổng máy ảo" value={String(stats.total)} />
        <StatCard icon={<Play size={18} />} label="Đang chạy" value={String(stats.running)} />
        <StatCard
          icon={<Activity size={18} />}
          label="Đang dừng"
          value={String(stats.total - stats.running)}
        />
        <StatCard icon={<HardDrive size={18} />} label="Dung lượng" value={formatBytes(stats.disk)} />
      </div>

      {/* Warm pool */}
      <div className="mt-4 rounded-lg border border-border bg-surface p-5 shadow-soft">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Flame size={18} className="text-warning" />
            <span className="font-medium">Warm pool (VM nóng)</span>
          </div>
          <Button size="sm" variant="secondary" disabled={refilling} onClick={refill}>
            <RefreshCw size={15} className={refilling ? 'animate-spin' : ''} />
            Nạp pool
          </Button>
        </div>
        <div className="mt-3 flex items-baseline gap-2">
          <span className="text-2xl font-semibold">{poolSize}</span>
          <span className="text-sm text-fg-muted">
            / mục tiêu {target || '—'} · lấy tức thì (0s cold-boot)
          </span>
        </div>
        <p className="mt-1 text-xs text-fg-muted">
          {target > 0
            ? `Tự động giữ ${target} VM nóng (base #${base ?? '?'}). Bật/đổi trong Settings.`
            : 'Đặt "Số VM giữ nóng" > 0 trong Settings để tự động refill nền.'}
        </p>
      </div>
    </div>
  );
}
