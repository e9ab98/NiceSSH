import { ipc } from './client';

export interface HostBlock {
  label: string;
  isMatch: boolean;
  directives: [string, string][];
  managed: boolean;
  startLine: number;
  endLine: number;
}

export const getSshConfig = () => ipc<HostBlock[]>('get_ssh_config');
export const validateSshConfig = () =>
  ipc<{ ok: boolean; summary: string; details: string }>('validate_ssh_config');
