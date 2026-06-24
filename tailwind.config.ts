import type { Config } from 'tailwindcss';

export default {
  darkMode: ['class', '[data-theme="dark"]'],
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        'bg-0': 'var(--bg-0)',
        'bg-1': 'var(--bg-1)',
        'bg-2': 'var(--bg-2)',
        border: 'var(--border)',
        'border-strong': 'var(--border-strong)',
        'text-0': 'var(--text-0)',
        'text-1': 'var(--text-1)',
        'text-2': 'var(--text-2)',
        accent: 'var(--accent)',
        'accent-hover': 'var(--accent-hover)',
        brand: 'var(--brand)',
        'brand-strong': 'var(--brand-strong)',
        'brand-soft': 'var(--brand-soft)',
        'brand-hot': 'var(--brand-hot)',
        ink: 'var(--ink)',
        success: 'var(--success)',
        warning: 'var(--warning)',
        danger: 'var(--danger)',
        info: 'var(--info)',
      },
      borderRadius: {
        md: '10px',
        lg: '14px',
        '2xl': '18px',
      },
      boxShadow: {
        card: 'var(--panel-shadow)',
        'card-hover': '0 14px 28px rgba(15, 23, 42, 0.1), inset 0 1px rgba(255, 255, 255, 0.92)',
        button: 'var(--button-shadow)',
        'button-hover': 'var(--button-hover-shadow)',
      },
      fontFamily: {
        sans: ['var(--font-ui)'],
        mono: ['var(--font-mono)'],
      },
    },
  },
  plugins: [],
} satisfies Config;
