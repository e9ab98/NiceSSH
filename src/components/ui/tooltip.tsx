import * as React from 'react';
import * as TooltipPrimitive from '@radix-ui/react-tooltip';
import { cn } from '../../lib/utils';

export const TooltipProvider: React.FC<React.ComponentPropsWithoutRef<typeof TooltipPrimitive.Provider>> = (props) => (
  <TooltipPrimitive.Provider delayDuration={300} {...props} />
);

export const Tooltip = TooltipPrimitive.Root;
export const TooltipTrigger = TooltipPrimitive.Trigger;

export const TooltipContent = React.forwardRef<
  React.ElementRef<typeof TooltipPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TooltipPrimitive.Content>
>(({ className, sideOffset = 6, ...props }, ref) => (
  <TooltipPrimitive.Portal>
    <TooltipPrimitive.Content
      ref={ref}
      sideOffset={sideOffset}
      className={cn(
        'z-50 overflow-hidden rounded-md bg-ink text-white px-2.5 py-1.5 text-xs font-medium',
        'shadow-[0_8px_18px_rgba(15,23,42,0.18)]',
        'data-[state=delayed-open]:anim-fade',
        className
      )}
      {...props}
    />
  </TooltipPrimitive.Portal>
));
TooltipContent.displayName = 'TooltipContent';
