import * as React from 'react';
import { cn } from '../../lib/utils';

export const Separator: React.FC<React.HTMLAttributes<HTMLDivElement> & { orientation?: 'horizontal' | 'vertical' }> = ({
  className, orientation = 'horizontal', ...props
}) => (
  <div
    role="separator"
    className={cn(
      'shrink-0 bg-border',
      orientation === 'horizontal' ? 'h-px w-full' : 'h-full w-px',
      className
    )}
    {...props}
  />
);
