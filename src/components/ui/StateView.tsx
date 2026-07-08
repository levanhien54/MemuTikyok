import type { ReactNode } from 'react';
import { Loader2 } from 'lucide-react';

/** Khối hiển thị trạng thái rỗng/đang tải/lỗi (§10.3 SRS). */
export function StateView({
  icon,
  title,
  description,
  action,
}: {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 p-12 text-center">
      {icon && <div className="text-fg-muted">{icon}</div>}
      <h3 className="text-base font-medium">{title}</h3>
      {description && <p className="max-w-sm text-sm text-fg-muted">{description}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function LoadingView({ label = 'Đang tải…' }: { label?: string }) {
  return <StateView icon={<Loader2 className="animate-spin" size={28} />} title={label} />;
}
