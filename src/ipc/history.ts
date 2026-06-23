import { ipc } from './client';

export interface HistoryIndexEntry {
  id: string;
  timestamp: string;
  operation: string;
  summary: string;
  fileCount: number;
}

export const listHistory = (limit = 50) => ipc<HistoryIndexEntry[]>('list_history', { limit });
export const rollback = (id: string) => ipc<void>('rollback', { entryId: id });
