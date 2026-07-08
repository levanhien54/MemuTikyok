import { forwardRef, type ButtonHTMLAttributes } from 'react';
import { cn } from '@/lib/cn';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';
type Size = 'sm' | 'md' | 'icon';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

const variants: Record<Variant, string> = {
  primary: 'bg-gradient-primary text-white shadow-glow hover:brightness-110 active:brightness-95',
  secondary: 'bg-surface-2 text-fg hover:bg-muted border border-border',
  ghost: 'text-fg-muted hover:text-fg hover:bg-surface-2',
  danger: 'bg-danger text-white hover:brightness-110 active:brightness-95',
};

const sizes: Record<Size, string> = {
  sm: 'h-8 px-3 text-sm gap-1.5',
  md: 'h-10 px-4 text-sm gap-2',
  icon: 'h-9 w-9 justify-center',
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'secondary', size = 'md', className, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(
        'inline-flex items-center rounded-md font-medium transition-all',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary',
        'disabled:opacity-50 disabled:pointer-events-none',
        variants[variant],
        sizes[size],
        className,
      )}
      {...props}
    />
  ),
);
Button.displayName = 'Button';
