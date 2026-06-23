import { useEffect, useRef, useState } from 'react';

export interface ContextMenuItem {
  label: string;
  onSelect: () => void;
  destructive?: boolean;
}

interface Props {
  /** Pixel position of the right-click. The menu auto-flips near viewport edges. */
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

export function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  // Flip near the right/bottom edges so the menu never falls off-screen.
  const [pos, setPos] = useState({ left: x, top: y });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left = x;
    let top = y;
    if (x + rect.width > vw) left = vw - rect.width - 4;
    if (y + rect.height > vh) top = vh - rect.height - 4;
    setPos({ left, top });
  }, [x, y]);

  return (
    <div
      ref={ref}
      role="menu"
      style={{ position: 'fixed', left: pos.left, top: pos.top, zIndex: 60 }}
      className="min-w-[160px] rounded-md border border-border bg-bg-1 shadow-md py-1"
    >
      {items.map((item, i) => (
        <button
          key={i}
          role="menuitem"
          onClick={() => { item.onSelect(); onClose(); }}
          className={`w-full text-left px-3 py-1.5 text-sm transition-colors ${
            item.destructive
              ? 'text-danger hover:bg-bg-2'
              : 'text-text-0 hover:bg-bg-2'
          }`}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
