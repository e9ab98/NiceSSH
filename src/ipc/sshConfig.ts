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

export const addManagedHostBlock = (params: {
  label: string;
  isMatch: boolean;
  directives: [string, string][];
}) => ipc<HostBlock>('add_managed_host_block', params);

export const updateManagedHostBlock = (params: {
  currentLabel: string;
  newLabel: string;
  isMatch: boolean;
  directives: [string, string][];
}) => ipc<HostBlock>('update_managed_host_block', params);

export const deleteManagedHostBlock = (label: string) =>
  ipc<void>('delete_managed_host_block', { label });
