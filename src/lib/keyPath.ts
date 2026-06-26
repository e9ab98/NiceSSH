/**
 * Helpers for working with the stored `identity.keyPath` field.
 *
 * The field historically stored a full file path (e.g. `~/.ssh/id_work`).
 * Newer code stores a *directory* (e.g. `~/.ssh/`) plus a separate
 * `identity.label` for the file name. These helpers smooth over both
 * representations so the form dialog can edit either kind of record
 * without the user noticing the migration.
 */

export function basename(p: string): string {
  if (!p) return '';
  // Strip trailing slashes then take the last segment. Works for both
  // /Users/x/.ssh/id_ed25519 and ~/work/ style paths.
  const trimmed = p.replace(/[\\/]+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
}

export function dirname(p: string): string {
  if (!p) return '';
  const trimmed = p.replace(/[\\/]+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return idx >= 0 ? trimmed.slice(0, idx + 1) : '';
}

/**
 * Whether a final path segment looks like an SSH key *file* (vs. a
 * folder). We treat a path as a file only when the basename matches a
 * known SSH key naming pattern. This is conservative — when in doubt we
 * treat the path as a directory so a stored value like `~/keys` or
 * `~/.ssh` doesn't get clobbered into `~/`.
 */
export function looksLikeKeyFile(seg: string): boolean {
  if (!seg) return false;
  if (seg.endsWith('.pub') || seg.endsWith('.key') || seg.endsWith('.pem')) return true;
  // Any segment that looks like a key file: starts with `id_` (e.g.
  // `id_rsa`, `id_ed25519`, `id_work`, `id_github`) or matches the
  // exact well-known default names. We accept the broader `id_*` form
  // because users commonly name keys after their use case.
  if (/^id_/.test(seg)) return true;
  return /^(id_rsa|id_ed25519|id_ecdsa|id_dsa|id_xmss)$/.test(seg);
}

/**
 * Split a stored `keyPath` into (directory, label). The stored value can
 * be one of:
 *   - A bare directory (new format): `~/keys/` or `~/keys` or `~/.ssh`
 *   - A full file path (old format): `~/keys/work_id_ed25519` — we split
 *     it into `~/keys/` + `work_id_ed25519`.
 *
 * **We do not normalise the trailing slash.** A stored value of
 * `/Users/x/.ssh/e9ab98-GitHub` (no trailing slash) round-trips as-is;
 * a stored value of `/Users/x/.ssh/e9ab98-GitHub/` round-trips with the
 * trailing slash. Forcing a trailing slash on read used to make the
 * field appear to gain a slash every time the user opened the form,
 * which read like data corruption.
 */
export function splitKeyPath(stored: string, fallbackLabel: string): { dir: string; label: string } {
  if (!stored) return { dir: '~/.ssh/', label: fallbackLabel };
  const trimmed = stored.replace(/[\\/]+$/, '');
  const lastSeg = basename(trimmed);
  if (!lastSeg) {
    return { dir: stored, label: fallbackLabel };
  }
  if (!looksLikeKeyFile(lastSeg)) {
    // The last segment is a regular folder name (e.g. ".ssh", "keys",
    // "my-keys") — the whole path is a directory. Preserve the
    // caller-provided trailing slash if any.
    return { dir: stored, label: fallbackLabel };
  }
  // Otherwise: looks like a file path, split into dir + label. Preserve
  // the caller's trailing slash on the directory portion.
  const dir = dirname(trimmed) || '~/.ssh/';
  const cleanLabel = lastSeg.endsWith('.pub') ? lastSeg.slice(0, -4) : lastSeg;
  return { dir, label: cleanLabel || fallbackLabel };
}

/**
 * Render a stored `keyPath` + `label` pair as a single display string
 * suitable for showing the user what file path the identity will use
 * (`/Users/x/.ssh/e9ab98-GitHub/e9ab98-GitHub`). Tolerates a keyPath
 * with or without a trailing slash so old data round-trips cleanly.
 */
export function joinKeyPath(keyPath: string, label: string): string {
  if (!keyPath) return label;
  return keyPath.endsWith('/') ? keyPath + label : keyPath + '/' + label;
}

export type KeyPathOwner = { keyPath: string; label: string };

/**
 * Return the full private-key file path for an identity (`<keyPath>/<label>`).
 * Use this anywhere the consumer (Rust command, ssh-add, ssh-keygen -f) needs
 * a *file* path rather than the directory stored on the identity.
 */
export function fullKeyPath(id: KeyPathOwner): string {
  return joinKeyPath(id.keyPath, id.label);
}

/** The label rule: letters, digits, dot, underscore, hyphen, plus. */
export function sanitizeLabel(s: string): string {
  return s.replace(/[\\/]+/g, '_');
}
