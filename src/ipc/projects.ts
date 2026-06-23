import { ipc } from './client';

export interface Project {
  id: string;
  name: string;
  path: string;
  identityId: string | null;
}

export const listProjects = () => ipc<Project[]>('list_projects');
export const addProject = (p: { name: string; path: string; identityId: string | null }) =>
  ipc<Project>('add_project', p);
export const removeProject = (id: string) => ipc<void>('remove_project', { id });
export const assignIdentity = (projectId: string, identityId: string) =>
  ipc<Project>('assign_identity', { projectId, identityId });
