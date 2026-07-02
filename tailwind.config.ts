import type { Config } from 'tailwindcss';

/**
 * Design tokens (§10.2 SRS). Màu điều khiển qua CSS variables trong index.css
 * để bật/tắt Dark/Light mà không đổi markup.
 */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Semantic tokens -> CSS variables (HSL, không kèm màu để dùng với /opacity)
        bg: 'hsl(var(--bg) / <alpha-value>)',
        surface: 'hsl(var(--surface) / <alpha-value>)',
        'surface-2': 'hsl(var(--surface-2) / <alpha-value>)',
        border: 'hsl(var(--border) / <alpha-value>)',
        muted: 'hsl(var(--muted) / <alpha-value>)',
        fg: 'hsl(var(--fg) / <alpha-value>)',
        'fg-muted': 'hsl(var(--fg-muted) / <alpha-value>)',
        primary: 'hsl(var(--primary) / <alpha-value>)',
        'primary-2': 'hsl(var(--primary-2) / <alpha-value>)',
        success: 'hsl(var(--success) / <alpha-value>)',
        warning: 'hsl(var(--warning) / <alpha-value>)',
        danger: 'hsl(var(--danger) / <alpha-value>)',
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 4px)',
        sm: 'calc(var(--radius) - 8px)',
      },
      boxShadow: {
        soft: '0 4px 24px -6px hsl(var(--shadow) / 0.25)',
        glow: '0 0 0 1px hsl(var(--primary) / 0.25), 0 8px 32px -8px hsl(var(--primary) / 0.35)',
      },
      fontFamily: {
        sans: ['Inter', 'Outfit', 'Roboto', 'system-ui', 'sans-serif'],
      },
      backgroundImage: {
        'gradient-primary': 'linear-gradient(135deg, hsl(var(--primary)), hsl(var(--primary-2)))',
      },
      keyframes: {
        'fade-in': {
          from: { opacity: '0', transform: 'translateY(4px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
      },
      animation: {
        'fade-in': 'fade-in 0.2s ease-out',
      },
    },
  },
  plugins: [],
} satisfies Config;
