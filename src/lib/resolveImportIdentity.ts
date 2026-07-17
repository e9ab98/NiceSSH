/**
 * Decides which identity the "Add Project" import flow should bind
 * to a freshly-imported repository, given:
 *   - what the repo's `.git/config` already says
 *   - whether the repo path is matched by an `includeIf "gitdir:..."`
 *     rule in `~/.gitconfig`
 *   - what NiceSSH has recorded as the user's global default identity
 *
 * Pure: no IPC, no side effects, no I/O. The caller is responsible
 * for fetching the three inputs (typically via Tauri's
 * `get_repo_git_config`, an includeIf resolver, and
 * `useGlobalDefaultStore.id`) and then reacting to the result
 * (`needsBinding === true` ⇒ call `applyIdentityToRepo`).
 *
 * See: docs/superpowers/specs/2026-07-16-import-project-global-default-fallback-designs.md
 */

export type ImportIdentitySource =
  | 'project' // repo .git/config already has identity fields
  | 'includeIf' // repo path matched an includeIf gitdir: rule
  | 'globalDefault' // NiceSSH cfg.globalDefaultIdentityId
  | 'none'; // nothing to bind

export interface ResolvedImportIdentity {
  source: ImportIdentitySource;
  /**
   * Identity id to bind. `null` when `source === 'none'`. Also `null`
   * for `source === 'project'` — the repo already has whatever it
   * has, the caller does not need to know which NiceSSH identity it
   * corresponds to.
   */
  identityId: string | null;
  /**
   * Whether the caller should write project-level config to make the
   * chosen identity stick. Only `true` for `source === 'globalDefault'`.
   */
  needsBinding: boolean;
}

export interface ImportIdentityInput {
  /** Parsed `get_repo_git_config` output, or `null` if unread. */
  repoConfig: {
    userName?: string | null;
    userEmail?: string | null;
    sshCommand?: string | null;
  } | null;
  /**
   * Result of an includeIf match against the repo's absolute path.
   * `resolved: false` is the "not yet wired up" placeholder used by
   * the current implementation — see spec "已知缺口".
   */
  includeIfResult:
    | { resolved: true; identityId: string }
    | { resolved: false };
  /**
   * NiceSSH's recorded global default. `exists: false` means the
   * recorded id no longer resolves to a known identity (deleted,
   * corrupted, etc.) and must NOT be written into project config.
   */
  globalDefault: { identityId: string | null; exists: boolean } | null;
}

const hasNonEmpty = (s: string | null | undefined): boolean =>
  typeof s === 'string' && s.length > 0;

export function resolveImportIdentity(
  input: ImportIdentityInput,
): ResolvedImportIdentity {
  const { repoConfig, includeIfResult, globalDefault } = input;

  // Priority 1: project-level config (any non-empty field counts).
  if (
    repoConfig &&
    (hasNonEmpty(repoConfig.userName) ||
      hasNonEmpty(repoConfig.userEmail) ||
      hasNonEmpty(repoConfig.sshCommand))
  ) {
    return { source: 'project', identityId: null, needsBinding: false };
  }

  // Priority 2: includeIf hit. Caller decides what "hit" means; we
  // trust the { resolved: true, identityId } shape.
  if (includeIfResult.resolved) {
    return {
      source: 'includeIf',
      identityId: includeIfResult.identityId,
      needsBinding: false,
    };
  }

  // Priority 3: NiceSSH's recorded global default. We require both
  // a non-null id AND `exists === true` so a dangling pointer can
  // never get written into a repo.
  if (globalDefault && globalDefault.identityId && globalDefault.exists) {
    return {
      source: 'globalDefault',
      identityId: globalDefault.identityId,
      needsBinding: true,
    };
  }

  // Priority 4: nothing to bind.
  return { source: 'none', identityId: null, needsBinding: false };
}
