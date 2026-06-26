import { describe, it, expect } from 'vitest';
import { computeMatchPathSeed } from '../../../src/features/identitySwitcher/IdentitySwitcherDialog';

describe('computeMatchPathSeed', () => {
  it('returns the currently-bound matchPath when present', () => {
    expect(computeMatchPathSeed('~/work', '/Users/x/work/myrepo')).toBe('~/work');
  });

  it('trims whitespace from the existing matchPath', () => {
    expect(computeMatchPathSeed('  ~/work  ', null)).toBe('~/work');
  });

  it('ignores an empty currentMatchPath and falls back to projectPath', () => {
    expect(computeMatchPathSeed('', '/Users/x/work/myrepo')).toBe('/Users/x/work/myrepo/');
  });

  it('ignores a whitespace-only currentMatchPath and falls back to projectPath', () => {
    expect(computeMatchPathSeed('   ', '/Users/x/work/myrepo')).toBe('/Users/x/work/myrepo/');
  });

  it('falls back to the project path itself, not its parent directory', () => {
    // Regression: previously this returned '/Users/x/work/' (the parent),
    // but the user opened a single repo and expects a single-repo bind.
    expect(computeMatchPathSeed(null, '/Users/x/work/myrepo')).toBe('/Users/x/work/myrepo/');
  });

  it('preserves a trailing slash on projectPath', () => {
    expect(computeMatchPathSeed(null, '/Users/x/work/myrepo/')).toBe('/Users/x/work/myrepo/');
  });

  it('returns empty string when both inputs are missing', () => {
    expect(computeMatchPathSeed(null, null)).toBe('');
    expect(computeMatchPathSeed(undefined, undefined)).toBe('');
    expect(computeMatchPathSeed('', '')).toBe('');
  });
});
