import { ipc } from './client';

export interface EnvCheck {
  tool: string;
  status: 'ok' | 'missing' | 'warning';
  detail: string;
}

export const checkEnvironment = () => ipc<EnvCheck[]>('check_environment');
export const clearHistory = () => ipc<void>('clear_history');
export const readLogTail = (lines: number) => ipc<string>('read_log_tail', { lines });
export const clearLog = () => ipc<void>('clear_log');
