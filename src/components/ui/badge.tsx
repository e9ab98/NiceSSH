import * as React from 'react';
import { cn } from '../../lib/utils';

interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: 'default' | 'outline' | 'success' | 'warning' | 'danger';
}

export const Badge: React.FC<BadgeProps> = ({ className, variant = 'default', ...props }) => {
  const styles = {
    default: 'bg-accent text-bg-0',
    outline: 'border border-border text-text-0',
    success: 'bg-success text-bg-0',
    warning: 'bg-warning text-bg-0',
    danger: 'bg-danger text-bg-0',
  }[variant];
  return <div className={cn('inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium', styles, className)} {...props} />;
};
