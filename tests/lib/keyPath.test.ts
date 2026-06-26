import { describe, it, expect } from 'vitest';
import {
  basename,
  dirname,
  fullKeyPath,
  joinKeyPath,
  looksLikeKeyFile,
  sanitizeLabel,
  splitKeyPath,
} from '../../src/lib/keyPath';

describe('basename', () => {
  it('returns file name for unix paths', () => {
    expect(basename('~/.ssh/id_ed25519')).toBe('id_ed25519');
    expect(basename('/Users/x/.ssh/id_work')).toBe('id_work');
  });
  it('strips trailing slashes', () => {
    expect(basename('~/.ssh/')).toBe('.ssh');
    expect(basename('~/work///')).toBe('work');
  });
  it('handles windows separators', () => {
    expect(basename('C:\\Users\\x\\.ssh\\id_rsa')).toBe('id_rsa');
  });
  it('returns empty for empty / slash-only input', () => {
    expect(basename('')).toBe('');
    expect(basename('/')).toBe('');
  });
});

describe('dirname', () => {
  it('returns parent dir with trailing slash', () => {
    expect(dirname('~/.ssh/id_ed25519')).toBe('~/.ssh/');
    expect(dirname('/Users/x/.ssh/id_work')).toBe('/Users/x/.ssh/');
  });
  it('handles bare directory inputs', () => {
    expect(dirname('~/.ssh/')).toBe('~/');
    expect(dirname('~/.ssh')).toBe('~/');
  });
});

describe('looksLikeKeyFile', () => {
  it('matches .pub / .key / .pem', () => {
    expect(looksLikeKeyFile('id_work.pub')).toBe(true);
    expect(looksLikeKeyFile('server.key')).toBe(true);
    expect(looksLikeKeyFile('legacy.pem')).toBe(true);
  });
  it('matches well-known SSH key basenames', () => {
    expect(looksLikeKeyFile('id_rsa')).toBe(true);
    expect(looksLikeKeyFile('id_ed25519')).toBe(true);
    expect(looksLikeKeyFile('id_ecdsa')).toBe(true);
    expect(looksLikeKeyFile('id_dsa')).toBe(true);
  });
  it('matches user-named keys that start with id_', () => {
    expect(looksLikeKeyFile('id_work')).toBe(true);
    expect(looksLikeKeyFile('id_github')).toBe(true);
    expect(looksLikeKeyFile('id_company_2024')).toBe(true);
  });
  it('does not match regular folder names', () => {
    expect(looksLikeKeyFile('.ssh')).toBe(false);
    expect(looksLikeKeyFile('keys')).toBe(false);
    expect(looksLikeKeyFile('my-keys')).toBe(false);
    expect(looksLikeKeyFile('')).toBe(false);
  });
  it('does not match generic filenames', () => {
    expect(looksLikeKeyFile('work_id_ed25519')).toBe(false);
    expect(looksLikeKeyFile('github-deploy')).toBe(false);
    expect(looksLikeKeyFile('deploy')).toBe(false);
  });
});

describe('splitKeyPath — no trailing-slash normalisation', () => {
  it('returns default dir for empty input', () => {
    expect(splitKeyPath('', 'id_ed25519')).toEqual({ dir: '~/.ssh/', label: 'id_ed25519' });
  });

  it('preserves a trailing slash on a bare directory', () => {
    expect(splitKeyPath('~/.ssh/', 'id_ed25519')).toEqual({ dir: '~/.ssh/', label: 'id_ed25519' });
  });

  it('preserves *no* trailing slash on a bare directory (regression: used to force-add one)', () => {
    // Previously this would have returned { dir: '~/.ssh/', label: 'id_ed25519' },
    // making the keyPath appear to gain a slash every time the user opened
    // the edit form.
    expect(splitKeyPath('~/.ssh', 'id_ed25519')).toEqual({ dir: '~/.ssh', label: 'id_ed25519' });
  });

  it('preserves a custom folder name (no trailing slash)', () => {
    expect(splitKeyPath('~/keys/custom', 'id_ed25519')).toEqual({
      dir: '~/keys/custom',
      label: 'id_ed25519',
    });
  });

  it('preserves a custom folder name (with trailing slash)', () => {
    expect(splitKeyPath('~/keys/custom/', 'id_ed25519')).toEqual({
      dir: '~/keys/custom/',
      label: 'id_ed25519',
    });
  });

  it('splits a full file path (old format) into dir + label — no slash on returned dir', () => {
    expect(splitKeyPath('~/.ssh/id_work', 'fallback')).toEqual({ dir: '~/.ssh/', label: 'id_work' });
  });

  it('splits a .pub full file path into dir + label without extension', () => {
    expect(splitKeyPath('~/.ssh/id_work.pub', 'fallback')).toEqual({ dir: '~/.ssh/', label: 'id_work' });
  });

  it('handles absolute paths', () => {
    expect(splitKeyPath('/Users/alice/.ssh/id_rsa', 'fallback')).toEqual({
      dir: '/Users/alice/.ssh/',
      label: 'id_rsa',
    });
  });

  it('regression: /Users/.../e9ab98-GitHub (no trailing slash) round-trips unchanged', () => {
    // This is the case the user reported: opening the edit form should
    // not appear to "add" a trailing slash to the path.
    const r = splitKeyPath('/Users/gaoxin/.ssh/e9ab98-GitHub', 'e9ab98-GitHub');
    expect(r).toEqual({ dir: '/Users/gaoxin/.ssh/e9ab98-GitHub', label: 'e9ab98-GitHub' });
  });

  it('regression: /Users/.../e9ab98-GitHub/ (with trailing slash) round-trips unchanged', () => {
    const r = splitKeyPath('/Users/gaoxin/.ssh/e9ab98-GitHub/', 'e9ab98-GitHub');
    expect(r).toEqual({ dir: '/Users/gaoxin/.ssh/e9ab98-GitHub/', label: 'e9ab98-GitHub' });
  });
});

describe('joinKeyPath', () => {
  it('inserts a slash when keyPath has no trailing slash', () => {
    expect(joinKeyPath('/Users/x/.ssh/work', 'id_work'))
      .toBe('/Users/x/.ssh/work/id_work');
  });
  it('does not double the slash when keyPath already ends in /', () => {
    expect(joinKeyPath('/Users/x/.ssh/work/', 'id_work'))
      .toBe('/Users/x/.ssh/work/id_work');
  });
  it('handles empty keyPath (returns label only)', () => {
    expect(joinKeyPath('', 'id_work')).toBe('id_work');
  });
  it('preserves the user-supplied trailing slash on the directory part', () => {
    // Both "with /" and "without /" inputs render to the same display
    // string so old records look correct after a round trip.
    expect(joinKeyPath('/Users/x/.ssh/work/', 'id'))
      .toBe(joinKeyPath('/Users/x/.ssh/work', 'id'));
  });
});

describe('fullKeyPath', () => {
  it('joins dir + label with a single slash', () => {
    expect(fullKeyPath({ keyPath: '/Users/x/.ssh/e9ab98-GitHub', label: 'id_work' }))
      .toBe('/Users/x/.ssh/e9ab98-GitHub/id_work');
  });
  it('does not double the slash when dir already ends with /', () => {
    expect(fullKeyPath({ keyPath: '/Users/x/.ssh/e9ab98-GitHub/', label: 'id_work' }))
      .toBe('/Users/x/.ssh/e9ab98-GitHub/id_work');
  });
});

describe('sanitizeLabel', () => {
  it('replaces path separators with underscores', () => {
    expect(sanitizeLabel('foo/bar')).toBe('foo_bar');
    expect(sanitizeLabel('a\\b')).toBe('a_b');
  });
  it('passes through clean labels', () => {
    expect(sanitizeLabel('id_work-2024')).toBe('id_work-2024');
    expect(sanitizeLabel('deploy.key')).toBe('deploy.key');
  });
});
