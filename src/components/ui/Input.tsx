import { forwardRef, useState, type InputHTMLAttributes } from 'react';
import { Eye, EyeOff } from 'lucide-react';
import { cn } from '@/lib/cn';

interface FieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string;
  /** Ô mật khẩu: che mặc định, có nút hiện/ẩn. */
  secret?: boolean;
}

/**
 * Ô nhập có nhãn. Với `secret`, giá trị bị che và có toggle hiện/ẩn.
 * ⚠️ Giá trị nhạy cảm KHÔNG được log ở bất kỳ đâu (SEC-3/SEC-6 §9 SRS).
 */
export const Input = forwardRef<HTMLInputElement, FieldProps>(
  ({ label, secret, className, id, ...props }, ref) => {
    const [reveal, setReveal] = useState(false);
    const inputId = id ?? `f-${label.replace(/\s+/g, '-').toLowerCase()}`;

    return (
      <div className="flex flex-col gap-1.5">
        <label htmlFor={inputId} className="text-sm font-medium text-fg">
          {label}
        </label>
        <div className="relative">
          <input
            ref={ref}
            id={inputId}
            type={secret && !reveal ? 'password' : 'text'}
            autoComplete="off"
            spellCheck={false}
            className={cn(
              'h-10 w-full rounded-md border border-border bg-surface-2 px-3 text-sm outline-none transition-colors focus:border-primary',
              secret && 'pr-10',
              className,
            )}
            {...props}
          />
          {secret && (
            <button
              type="button"
              tabIndex={-1}
              onClick={() => setReveal((r) => !r)}
              aria-label={reveal ? 'Ẩn' : 'Hiện'}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-fg-muted hover:text-fg"
            >
              {reveal ? <EyeOff size={16} /> : <Eye size={16} />}
            </button>
          )}
        </div>
      </div>
    );
  },
);
Input.displayName = 'Input';
