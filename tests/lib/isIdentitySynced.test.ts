import { describe, it, expect } from 'vitest';
import { isIdentitySynced } from '../../src/lib/isIdentitySynced';

describe('isIdentitySynced', () => {
  describe('returns null (no judgement) for non-tracked states', () => {
    it('returns null for kind=none', () => {
      expect(isIdentitySynced({ kind: 'none' }, null)).toBeNull();
    });
    it('returns null for kind=user-only', () => {
      expect(isIdentitySynced({ kind: 'user-only' }, null)).toBeNull();
    });
    it('returns null for kind=untracked', () => {
      expect(isIdentitySynced(
        { kind: 'untracked', keyPath: '/tmp/k' },
        null,
      )).toBeNull();
    });
    it('returns null when tracked but repoConfig is null (IPC still loading)', () => {
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: 'a', userEmail: 'b' } as any, source: 'config' },
        null,
      )).toBeNull();
    });
  });

  describe('returns true when identity matches repoConfig', () => {
    it('matches when userName AND userEmail are equal', () => {
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: 'Alice', userEmail: 'a@b' } as any, source: 'config' },
        { userName: 'Alice', userEmail: 'a@b' },
      )).toBe(true);
    });
    it('matches when repoConfig fields are missing but identity has them (treated as in-sync: nothing to compare against)', () => {
      // If .git/config has neither name nor email, we can't claim a
      // mismatch — the user just hasn't committed yet. No warning.
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: 'Alice', userEmail: 'a@b' } as any, source: 'config' },
        { userName: null, userEmail: null },
      )).toBe(true);
    });
    it('trims whitespace before comparing', () => {
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: '  Alice  ', userEmail: ' a@b ' } as any, source: 'config' },
        { userName: 'Alice', userEmail: 'a@b' },
      )).toBe(true);
    });
  });

  describe('returns false when identity disagrees with repoConfig', () => {
    it('flags userName mismatch', () => {
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: 'Alice', userEmail: 'a@b' } as any, source: 'config' },
        { userName: 'Bob', userEmail: 'a@b' },
      )).toBe(false);
    });
    it('flags userEmail mismatch', () => {
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: 'Alice', userEmail: 'a@b' } as any, source: 'config' },
        { userName: 'Alice', userEmail: 'other@b' },
      )).toBe(false);
    });
    it('flags when identity has a name but repoConfig has none', () => {
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: 'Alice', userEmail: 'a@b' } as any, source: 'config' },
        { userName: null, userEmail: 'a@b' },
      )).toBe(false);
    });
    it('is case-sensitive (git itself is case-sensitive)', () => {
      expect(isIdentitySynced(
        { kind: 'tracked', identity: { userName: 'alice', userEmail: 'a@b' } as any, source: 'config' },
        { userName: 'Alice', userEmail: 'a@b' },
      )).toBe(false);
    });
  });
});
