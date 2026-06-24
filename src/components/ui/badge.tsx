import * as React from 'react';
import { cn } from '../../lib/utils';

type BadgeVariant = 'default' | 'outline' | 'success' | 'warning' | 'danger';

interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: BadgeVariant;
}

const styles: Record<BadgeVariant, string> = {
  default: 'bg-brand-soft text-brand-strong [[data-theme=dark]_&]:text-[#93c5fd]',
  success: 'bg-[rgba(34,197,94,0.1)] text-[#16a34a] [[data-theme=dark]_&]:bg-[rgba(74,222,128,0.12)] [[data-theme=dark]_&]:text-[#86efac]',
  warning: 'bg-[rgba(245,158,11,0.12)] text-[#b45309] [[data-theme=dark]_&]:bg-[rgba(251,191,36,0.14)] [[data-theme=dark]_&]:text-[#fcd34d]',
  danger: 'bg-[rgba(239,68,68,0.1)] text-[#dc2626] [[data-theme=dark]_&]:bg-[rgba(251,113,133,0.12)] [[data-theme=dark]_&]:text-[#fca5a5]',
  outline: 'border border-border text-text-0',
};

export const Badge: React.FC<BadgeProps> = ({ className, variant = 'default', ...props }) => (
  <div
    className={cn(
      'inline-flex items-center rounded-full px-2.5 py-0.5 text-[11px] font-bold',
      styles[variant],
      className
    )}
    {...props}
  />
);
