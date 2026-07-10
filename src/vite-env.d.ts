/// <reference types="vite/client" />

// Ambient module declaration for @tauri-apps/plugin-process. Once the
// package is installed (pnpm install) its real .d.ts will shadow this.
// Until then this lets tsc resolve the import.
declare module '@tauri-apps/plugin-process' {
  export function relaunch(): Promise<void>;
  export function exit(code?: number): void;
}
