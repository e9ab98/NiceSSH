import * as React from 'react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import { cn } from '../../lib/utils';

export const Dialog = DialogPrimitive.Root;
export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogPortal = DialogPrimitive.Portal;
export const DialogClose = DialogPrimitive.Close;

export const DialogOverlay: React.FC<React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>> = ({ className, ...props }) => (
  <DialogPrimitive.Overlay
    className={cn(
      'fixed inset-0 z-50 bg-[var(--overlay-scrim)] backdrop-blur-sm data-[state=open]:anim-fade data-[state=closed]:anim-fade',
      className
    )}
    {...props}
  />
);

export const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content>
>(({ className, children, ...props }, ref) => (
  <DialogPortal>
    <DialogOverlay />
    <DialogPrimitive.Content
      ref={ref}
      className={cn(
        'fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4',
        'rounded-2xl border border-border bg-bg-1 p-6 shadow-card data-[state=open]:anim-rise',
        className
      )}
      {...props}
    >
      {children}
      <DialogPrimitive.Close className="absolute right-4 top-4 text-text-1 hover:text-text-0 transition-colors">
        <X className="h-4 w-4" />
        <span className="sr-only">Close</span>
      </DialogPrimitive.Close>
    </DialogPrimitive.Content>
  </DialogPortal>
));
DialogContent.displayName = 'DialogContent';

export const DialogHeader: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ className, ...props }) => (
  <div className={cn('flex flex-col space-y-1.5', className)} {...props} />
);
export const DialogTitle: React.FC<React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>> = ({ className, ...props }) => (
  <DialogPrimitive.Title className={cn('text-lg font-bold text-text-0', className)} {...props} />
);
export const DialogDescription: React.FC<React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>> = ({ className, ...props }) => (
  <DialogPrimitive.Description className={cn('text-sm text-text-1', className)} {...props} />
);
export const DialogFooter: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ className, ...props }) => (
  <div className={cn('flex justify-end gap-2', className)} {...props} />
);
