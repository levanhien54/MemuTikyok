import { useSettingsStore } from '@/store/useSettingsStore';
import { Button } from '@/components/ui/Button';

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-border py-4">
      <div>
        <div className="text-sm font-medium">{label}</div>
        {hint && <div className="mt-0.5 text-xs text-fg-muted">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function SettingsView() {
  const settings = useSettingsStore((s) => s.settings);
  const save = useSettingsStore((s) => s.save);

  if (!settings) return null;

  return (
    <div className="flex-1 overflow-auto p-6">
      <h1 className="mb-6 text-lg font-semibold">Settings</h1>
      <div className="max-w-2xl rounded-lg border border-border bg-surface px-6 shadow-soft">
        <Field
          label="Đường dẫn MuMu"
          hint="Tự dò nếu để trống. Trỏ tới THƯ MỤC cài MuMu (bản bất kỳ, kể cả Pro) hoặc file MuMuManager.exe để chọn build muốn dùng."
        >
          <input
            value={settings.mumuPath ?? ''}
            placeholder="Tự động dò… hoặc D:\...\MuMu"
            onChange={(e) => void save({ mumuPath: e.target.value || null })}
            className="h-9 w-72 rounded-md border border-border bg-surface-2 px-3 text-sm outline-none focus:border-primary"
          />
        </Field>

        <Field
          label="Đường dẫn APK TikTok"
          hint="Để trống = dùng mặc định. Người dùng có thể trỏ tới file .apk khác."
        >
          <input
            value={settings.tiktokApkPath ?? ''}
            placeholder="D:\MemuTiktok\appTiktok\tiktok-40-0-0.apk"
            onChange={(e) => void save({ tiktokApkPath: e.target.value || null })}
            className="h-9 w-72 rounded-md border border-border bg-surface-2 px-3 text-sm outline-none focus:border-primary"
          />
        </Field>

        <Field
          label="Magisk APK (khóa model)"
          hint="Trỏ tới file Magisk-v30.x.apk. MPM trích resetprop để KHÓA model/fingerprint (chống MuMu ghi đè). Để trống = tắt khóa model."
        >
          <input
            value={settings.magiskApkPath ?? ''}
            placeholder="D:\MemuTiktok\appTiktok\Magisk-v30.7.apk"
            onChange={(e) => void save({ magiskApkPath: e.target.value || null })}
            className="h-9 w-72 rounded-md border border-border bg-surface-2 px-3 text-sm outline-none focus:border-primary"
          />
        </Field>

        <Field label="Chu kỳ polling (ms)" hint="Tần suất cập nhật trạng thái (1000–10000ms).">
          <input
            type="number"
            min={1000}
            max={10000}
            step={500}
            value={settings.pollIntervalMs}
            onChange={(e) => void save({ pollIntervalMs: Number(e.target.value) })}
            className="h-9 w-32 rounded-md border border-border bg-surface-2 px-3 text-sm outline-none focus:border-primary"
          />
        </Field>

        <Field label="Số lệnh song song tối đa" hint="Giới hạn tải khi chạy/dừng nhiều profile.">
          <input
            type="number"
            min={1}
            max={10}
            value={settings.maxConcurrency}
            onChange={(e) => void save({ maxConcurrency: Number(e.target.value) })}
            className="h-9 w-32 rounded-md border border-border bg-surface-2 px-3 text-sm outline-none focus:border-primary"
          />
        </Field>

        <Field label="Giao diện">
          <div className="flex gap-2">
            <Button
              size="sm"
              variant={settings.theme === 'dark' ? 'primary' : 'secondary'}
              onClick={() => void save({ theme: 'dark' })}
            >
              Dark
            </Button>
            <Button
              size="sm"
              variant={settings.theme === 'light' ? 'primary' : 'secondary'}
              onClick={() => void save({ theme: 'light' })}
            >
              Light
            </Button>
          </div>
        </Field>
      </div>
    </div>
  );
}
