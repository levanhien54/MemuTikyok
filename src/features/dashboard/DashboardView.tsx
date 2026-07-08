import { useMemo } from 'react';
import { Activity, Play, Users } from 'lucide-react';
import { useProfileStore } from '@/store/useProfileStore';

function StatCard({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
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

/**
 * Dashboard theo mô hình PROFILE (disposable): thống kê từ danh sách profile —
 * VM chỉ là tài nguyên tạm khi Chạy, nên không thống kê VM riêng lẻ ở đây.
 */
export function DashboardView() {
  const profiles = useProfileStore((s) => s.profiles);

  const stats = useMemo(() => {
    const running = profiles.filter((v) => v.runningVm != null).length;
    return { total: profiles.length, running, idle: profiles.length - running };
  }, [profiles]);

  return (
    <div className="flex-1 overflow-auto p-6">
      <h1 className="mb-6 text-lg font-semibold">Dashboard</h1>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <StatCard
          icon={<Users size={18} />}
          label="Tổng tài khoản (profile)"
          value={String(stats.total)}
        />
        <StatCard icon={<Play size={18} />} label="Đang chạy (VM)" value={String(stats.running)} />
        <StatCard icon={<Activity size={18} />} label="Đang nghỉ" value={String(stats.idle)} />
      </div>
      <p className="mt-4 max-w-2xl text-sm text-fg-muted">
        Kiến trúc <b>dùng-một-lần</b>: mỗi profile là dữ liệu bền (tài khoản + fingerprint); máy ảo
        chỉ được cấp khi bấm <b>Chạy</b> và bị hủy khi <b>Dừng</b> (backup phiên trước). Tối đa 5 VM
        chạy đồng thời để bảo vệ RAM/đĩa.
      </p>
    </div>
  );
}
