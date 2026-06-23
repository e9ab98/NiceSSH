import { ipc } from './client';

export interface EnvCheck {
  tool: string;
  status: 'ok' | 'missing' | 'warning';
  detail: string;
}

export const checkEnvironment = () => ipc<EnvCheck[]>('check_environment');
export const clearHistory = () => ipc<void>('clear_history');
