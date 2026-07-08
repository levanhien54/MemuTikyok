import { AnimatePresence, motion } from 'framer-motion';
import { Fingerprint, Copy, X } from 'lucide-react';
import type { HardwareProfile } from '@/types/instance';
import { Button } from '@/components/ui/Button';
import { toast } from '@/store/useToastStore';

interface Props {
  open: boolean;
  accountName: string;
  /** undefined = đang tải; null = chưa có fingerprint; object = dữ liệu. */
  hardware: HardwareProfile | null | undefined;
  onClose: () => void;
}

/** Một dòng nhãn–giá trị, có nút copy. */
function Row({ label, value }: { label: string; value: string }) {
  const copy = () => {
    void navigator.clipboard?.writeText(value).then(
      () => toast.success(`Đã copy ${label}`),
      () => toast.error('Không copy được'),
    );
  };
  return (
    <div className="flex items-center gap-3 border-b border-border/60 py-2 last:border-0">
      <span className="w-36 shrink-0 text-sm text-fg-muted">{label}</span>
      <span className="min-w-0 flex-1 truncate font-mono text-sm text-fg" title={value}>
        {value}
      </span>
      <button
        onClick={copy}
        title={`Copy ${label}`}
        className="shrink-0 rounded p-1.5 text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg"
      >
        <Copy size={14} />
      </button>
    </div>
  );
}

/**
 * Bảng thông tin thiết bị (fingerprint) đầy đủ của một tài khoản. Nạp trực tiếp từ
 * dữ liệu đã lưu (getHardware) — hiển thị nguyên vẹn để đối chiếu/copy.
 */
export function FingerprintDialog({ open, accountName, hardware, onClose }: Props) {
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={onClose}
        >
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label={`Fingerprint của ${accountName}`}
            className="w-full max-w-md rounded-lg border border-border bg-surface p-6 shadow-soft"
            initial={{ scale: 0.96, y: 10 }}
            animate={{ scale: 1, y: 0 }}
            exit={{ scale: 0.96, opacity: 0 }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-4 flex items-center gap-2">
              <span className="flex h-8 w-8 items-center justify-center rounded-md bg-gradient-primary text-white">
                <Fingerprint size={16} />
              </span>
              <div className="min-w-0 flex-1">
                <h2 className="truncate text-lg font-semibold" title={accountName}>
                  {accountName}
                </h2>
                <p className="text-xs text-fg-muted">Thông tin thiết bị (fingerprint)</p>
              </div>
              <button
                onClick={onClose}
                aria-label="Đóng"
                className="rounded p-1.5 text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg"
              >
                <X size={16} />
              </button>
            </div>

            {hardware === undefined ? (
              <p className="py-6 text-center text-sm text-fg-muted">Đang tải…</p>
            ) : hardware === null ? (
              <p className="py-6 text-center text-sm text-fg-muted">
                Tài khoản này chưa có fingerprint. Fingerprint được sinh khi tạo máy ảo.
              </p>
            ) : (
              <>
                <div className="rounded-md border border-border bg-surface-2/40 px-3">
                  <Row label="Model" value={hardware.model} />
                  <Row label="Hãng (brand)" value={hardware.brand} />
                  <Row label="Nhà sản xuất" value={hardware.manufacturer} />
                  {hardware.device ? (
                    <Row label="Device (codename)" value={hardware.device} />
                  ) : null}
                  <Row label="IMEI" value={hardware.imei} />
                  <Row label="Android ID" value={hardware.androidId} />
                  <Row label="MAC" value={hardware.mac} />
                  <Row
                    label="Độ phân giải"
                    value={`${hardware.resWidth} × ${hardware.resHeight}`}
                  />
                  <Row label="DPI" value={String(hardware.dpi)} />
                  {hardware.buildFingerprint ? (
                    <Row label="Build fingerprint" value={hardware.buildFingerprint} />
                  ) : null}
                </div>
                <p className="mt-3 text-[11px] leading-snug text-fg-muted">
                  ⚠️ Đây là fingerprint <b>đã lưu</b> cho tài khoản (áp khi khởi chạy). Riêng
                  <b> model</b> có thể bị MuMu ghi đè ngẫu nhiên khi VM boot — android_id & độ phân
                  giải là các trường thực sự có hiệu lực.
                </p>
              </>
            )}

            <div className="mt-5 flex justify-end">
              <Button variant="ghost" onClick={onClose}>
                Đóng
              </Button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
