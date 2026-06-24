import * as React from 'react';
import { cn } from '../../lib/utils';

export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(
  ({ className, type, ...props }, ref) => (
    <input
      ref={ref}
      type={type}
      className={cn(
        'flex h-9 w-full rounded-md border border-border bg-bg-0 px-3 py-1 text-sm text-text-0 placeholder:text-text-2',
        'transition-colors focus-visible:outline-none focus-visible:border-brand focus-visible:ring-2 focus-visible:ring-brand-soft',
        'disabled:opacity-50 disabled:cursor-not-allowed',
        className
      )}
      {...props}
    />
  )
);
Input.displayName = 'Input';

export const Textarea = React.forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement>>(
  ({ className, ...props }, ref) => (
    <textarea
      ref={ref}
      className={cn(
        'flex min-h-[80px] w-full rounded-md border border-border bg-bg-0 px-3 py-2.5 text-sm text-text-0 placeholder:text-text-2',
        'transition-colors focus-visible:outline-none focus-visible:border-brand focus-visible:ring-2 focus-visible:ring-brand-soft',
        'disabled:opacity-50 disabled:cursor-not-allowed resize-y',
        className
      )}
      {...props}
    />
  )
);
Textarea.displayName = 'Textarea';

export const Label: React.FC<React.LabelHTMLAttributes<HTMLLabelElement>> = ({ className, ...props }) => (
  <label className={cn('text-xs font-semibold text-text-1', className)} {...props} />
);
