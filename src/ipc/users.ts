import { ipc } from './client';

/**
 * A git user — a (name, email) pair that can be reused across
 * multiple identities. The "link" between a User and an Identity
 * is value-based: an Identity is considered linked to a User when
 * `identity.userName === user.name && identity.userEmail ===
 * user.email`. Mutations to a User (especially the email/name)
 * propagate to all linked identities on the backend.
 *
 * See `src-tauri/src/config_store.rs` (User struct + doc) and
 * `src-tauri/src/commands/user.rs` for the canonical rules.
 */
export interface User {
  id: string;
  name: string;
  email: string;
}

export interface UserInput {
  name: string;
  email: string;
}

export const listUsers = () => ipc<User[]>('list_users');

export const createUser = (i: UserInput) =>
  ipc<User>('create_user', { input: i });

export const updateUser = (id: string, updated: UserInput) =>
  ipc<User>('update_user', { id, updated });

export const deleteUser = (id: string) =>
  ipc<void>('delete_user', { id });

/**
 * Read `~/.gitconfig` top-level `[user]` block and create a User
 * from it if both `user.name` and `user.email` are present and
 * not already in the pool. Returns the list of newly created
 * users — empty when gitconfig is missing, has no [user] block,
 * or the pair is already known (idempotent).
 */
export const importUsersFromGitconfig = () =>
  ipc<User[]>('import_users_from_gitconfig');
