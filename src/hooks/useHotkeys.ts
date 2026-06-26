import { useEffect, useRef } from 'react';

export type Hotkey = string;
export type HotkeyHandler = (e: KeyboardEvent) => void;

/**
 * Normalize a hotkey string to a canonical form for comparison.
 *
 * Accepts "mod" as a synonym for the platform modifier (Cmd on macOS,
 * Ctrl on Windows/Linux). Tokens are joined with "+", modifiers and
 * the key sorted by a fixed order so the order of tokens in the input
 * string does not matter.
 */
export function normalize(hotkey: string, isMac: boolean): string {
  const tokens = hotkey
    .toLowerCase()
    .split('+')
    .map((t) => t.trim())
    .filter(Boolean);
  const out: string[] = [];
  let key = '';
  for (const t of tokens) {
    if (t === 'mod' || t === 'cmd' || t === 'meta') out.push('mod');
    else if (t === 'ctrl' || t === 'control') out.push('ctrl');
    else if (t === 'alt' || t === 'option') out.push('alt');
    else if (t === 'shift') out.push('shift');
    else key = t;
  }
  // On non-mac, treat "mod" the same as ctrl for matching (browsers
  // report Meta on macOS only when Cmd is held).
  void isMac;
  out.push(key);
  // Sort modifiers into a canonical order.
  const order = ['ctrl', 'mod', 'alt', 'shift'];
  const mods = out.filter((x) => order.includes(x)).sort((a, b) => order.indexOf(a) - order.indexOf(b));
  const keys = out.filter((x) => !order.includes(x));
  return [...mods, ...keys].join('+');
}

function eventMatches(e: KeyboardEvent, normalized: string): boolean {
  const tokens = normalized.split('+');
  const wantMod = tokens.includes('mod');
  const wantCtrl = tokens.includes('ctrl');
  const wantAlt = tokens.includes('alt');
  const wantShift = tokens.includes('shift');
  const wantKey = tokens[tokens.length - 1];
  // Map "mod" -> meta on mac, ctrl elsewhere; we also accept the literal ctrl key for cross-platform.
  const modPressed = e.metaKey || (wantMod && !e.ctrlKey ? e.metaKey : e.ctrlKey);
  if (wantMod && !modPressed && !e.metaKey && !e.ctrlKey) return false;
  if (wantCtrl && !e.ctrlKey) return false;
  if (wantAlt && !e.altKey) return false;
  if (wantShift && !e.shiftKey) return false;
  // Reject if an extra modifier is pressed that we did not ask for.
  if (!wantShift && e.shiftKey) return false;
  if (!wantAlt && e.altKey) return false;
  if (!wantMod && !wantCtrl && (e.metaKey || e.ctrlKey)) return false;
  return e.key.toLowerCase() === wantKey;
}

/**
 * Register a set of keyboard shortcuts that fire while the document
 * has focus. Inputs in form fields (input/textarea/select/contentEditable)
 * automatically opt out so users can type "1" or "k" inside fields
 * without triggering shortcuts.
 */
export function useHotkeys(map: Record<Hotkey, HotkeyHandler>): void {
  const ref = useRef(map);
  useEffect(() => {
    ref.current = map;
  }, [map]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Skip if the user is typing in a form field.
      const target = e.target as HTMLElement | null;
      if (target) {
        const tag = target.tagName;
        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
        if (target.isContentEditable) return;
      }
      const isMac = navigator.platform.toLowerCase().includes('mac');
      for (const [hk, fn] of Object.entries(ref.current)) {
        if (eventMatches(e, normalize(hk, isMac))) {
          e.preventDefault();
          fn(e);
          return;
        }
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);
}
