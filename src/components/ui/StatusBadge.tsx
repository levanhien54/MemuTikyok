import type { InstanceStatus } from '@/types/instance';
import { statusMeta } from '@/lib/format';
import { cn } from '@/lib/cn';

export function StatusBadge({ status }: { status: InstanceStatus }) {
  const meta = statusMeta(status);
  return (
    <span className="inline-flex items-center gap-2 text-sm">
      <span className={cn('h-2.5 w-2.5 rounded-full', meta.dot)} />
      <span className={meta.text}>{meta.label}</span>
    </span>
  );
}
