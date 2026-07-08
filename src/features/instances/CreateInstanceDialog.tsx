import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { UserPlus, Pencil } from 'lucide-react';
import type { AccountProfile } from '@/types/instance';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { useModalFocusTrap } from '@/components/ui/useModalFocusTrap';
import { COUNTRY_CODES, countryLabel } from '@/lib/country';

const EMPTY: AccountProfile = {
  tiktokUsername: '',
  tiktokPassword: '',
  twoFa: '',
  tiktokPasskey: '',
  email: '',
  emailPassword: '',
};

/** Giá trị điền sẵn khi sửa (bỏ trống = tạo mới). */
export interface InstanceFormInitial {
  account: AccountProfile;
  note: string;
  country: string | null;
}

interface Props {
  open: boolean;
  onCancel: () => void;
  onSubmit: (
    account: AccountProfile,
    note: string,
    country: string | null,
  ) => void | Promise<void>;
  /** 'create' (mặc định) hoặc 'edit'. */
  mode?: 'create' | 'edit';
  /** Điền sẵn form (chế độ sửa). */
  initial?: InstanceFormInitial | null;
}

/**
 * Dialog tạo/sửa VM: nhập tên máy ảo (= tài khoản TikTok) + thông tin đăng nhập,
 * quốc gia yêu cầu và ghi chú. Dùng chung cho cả "Tạo VM" lẫn "Chỉnh sửa thông tin".
 * ⚠️ Dữ liệu nhạy cảm — che mật khẩu, không log, persist phải mã hóa (§9 SRS).
 */
export function CreateInstanceDialog({
  open,
  onCancel,
  onSubmit,
  mode = 'create',
  initial,
}: Props) {
  const isEdit = mode === 'edit';
  const [form, setForm] = useState<AccountProfile>(EMPTY);
  const [note, setNote] = useState('');
  const [country, setCountry] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const prevOpen = useRef(false);
  const dialogRef = useRef<HTMLDivElement>(null);

  // Đồng bộ giá trị điền sẵn mỗi khi dialog chuyển từ đóng → mở.
  useEffect(() => {
    if (open && !prevOpen.current) {
      setForm(initial?.account ?? EMPTY);
      setNote(initial?.note ?? '');
      setCountry(initial?.country ?? '');
      setSubmitting(false);
    }
    prevOpen.current = open;
  }, [open, initial]);

  const set = (key: keyof AccountProfile) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm((f) => ({ ...f, [key]: e.target.value }));

  const usernameValid = form.tiktokUsername.trim().length > 0;

  const submit = async () => {
    if (!usernameValid || submitting) return;
    setSubmitting(true);
    try {
      await onSubmit(
        { ...form, tiktokUsername: form.tiktokUsername.trim() },
        note.trim(),
        country || null,
      );
    } finally {
      setSubmitting(false);
    }
  };

  const cancel = () => {
    if (submitting) return;
    onCancel();
  };

  useModalFocusTrap(open, dialogRef, cancel);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={cancel}
        >
          <motion.div
            ref={dialogRef}
            role="dialog"
            aria-modal="true"
            aria-label={isEdit ? 'Chỉnh sửa thông tin tài khoản' : 'Tạo profile mới'}
            tabIndex={-1}
            className="w-full max-w-lg rounded-lg border border-border bg-surface p-6 shadow-soft"
            initial={{ scale: 0.96, y: 10 }}
            animate={{ scale: 1, y: 0 }}
            exit={{ scale: 0.96, opacity: 0 }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-1 flex items-center gap-2">
              <span className="flex h-8 w-8 items-center justify-center rounded-md bg-gradient-primary text-white">
                {isEdit ? <Pencil size={16} /> : <UserPlus size={16} />}
              </span>
              <h2 className="text-lg font-semibold">
                {isEdit ? 'Chỉnh sửa thông tin' : 'Tạo profile mới'}
              </h2>
            </div>
            <p className="mb-5 text-sm text-fg-muted">
              {isEdit
                ? 'Cập nhật tài khoản, quốc gia yêu cầu và ghi chú. Thông tin đăng nhập được lưu cục bộ và mã hóa.'
                : 'Chỉ tạo hồ sơ (dữ liệu tài khoản) — chưa cấp máy ảo. VM được cấp từ pool khi bấm Chạy. Thông tin đăng nhập lưu cục bộ và mã hóa.'}
            </p>

            <form
              className="flex flex-col gap-4"
              onSubmit={(e) => {
                e.preventDefault();
                void submit();
              }}
            >
              <Input
                label="Tài khoản TikTok (tên profile) *"
                placeholder="vd: tiktok_minh"
                value={form.tiktokUsername}
                onChange={set('tiktokUsername')}
                autoFocus
                required
              />

              <div className="grid grid-cols-2 gap-4">
                <Input
                  label="Mật khẩu TikTok"
                  secret
                  value={form.tiktokPassword}
                  onChange={set('tiktokPassword')}
                />
                <Input
                  label="2FA (khóa/secret)"
                  secret
                  value={form.twoFa}
                  onChange={set('twoFa')}
                />
              </div>

              <Input
                label="Passkey TikTok"
                secret
                value={form.tiktokPasskey}
                onChange={set('tiktokPasskey')}
              />

              <div className="grid grid-cols-2 gap-4">
                <Input
                  label="Email"
                  placeholder="email@example.com"
                  value={form.email}
                  onChange={set('email')}
                />
                <Input
                  label="Mật khẩu Email"
                  secret
                  value={form.emailPassword}
                  onChange={set('emailPassword')}
                />
              </div>

              <div className="grid grid-cols-2 gap-4">
                <div className="flex flex-col gap-1.5">
                  <label htmlFor="form-country" className="text-sm font-medium text-fg">
                    Quốc gia yêu cầu
                  </label>
                  <select
                    id="form-country"
                    value={country}
                    onChange={(e) => setCountry(e.target.value)}
                    className="h-10 w-full rounded-md border border-border bg-surface-2 px-3 text-sm outline-none transition-colors focus:border-primary"
                  >
                    <option value="">Không ràng buộc</option>
                    {COUNTRY_CODES.map((code) => (
                      <option key={code} value={code}>
                        {countryLabel(code)}
                      </option>
                    ))}
                  </select>
                  <span className="text-[11px] text-fg-muted">
                    Chỉ khởi chạy khi IP thoát thực tế khớp quốc gia này.
                  </span>
                </div>

                <div className="flex flex-col gap-1.5">
                  <label htmlFor="form-note" className="text-sm font-medium text-fg">
                    Ghi chú
                  </label>
                  <input
                    id="form-note"
                    value={note}
                    onChange={(e) => setNote(e.target.value)}
                    placeholder="Ghi chú tùy ý…"
                    autoComplete="off"
                    className="h-10 w-full rounded-md border border-border bg-surface-2 px-3 text-sm outline-none transition-colors focus:border-primary"
                  />
                </div>
              </div>

              <div className="mt-2 flex justify-end gap-3">
                <Button type="button" variant="ghost" onClick={cancel} disabled={submitting}>
                  Hủy
                </Button>
                <Button type="submit" variant="primary" disabled={!usernameValid || submitting}>
                  {submitting ? 'Đang lưu...' : isEdit ? 'Lưu thay đổi' : 'Tạo profile'}
                </Button>
              </div>
            </form>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
