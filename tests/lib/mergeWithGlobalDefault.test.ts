import { describe, it, expect } from 'vitest';
import { mergeWithGlobalDefault } from '../../src/lib/mergeWithGlobalDefault';

describe('mergeWithGlobalDefault', () => {
  describe('does nothing when repoConfig already has values', () => {
    it('keeps userName and fills missing userEmail from global default', () => {
      const out = mergeWithGlobalDefault(
        { userName: 'Alice', userEmail: null, userNameSource: 'project', userEmailSource: 'none' },
        { globalDefaultId: 'id-x', globalDefault: { userName: 'Bob', userEmail: 'b@x' } },
      );
      expect(out.userName).toBe('Alice');
      expect(out.userNameSource).toBe('project');
      // userEmail was null → filled from global default
      expect(out.userEmail).toBe('b@x');
      expect(out.userEmailSource).toBe('globalDefault');
    });

    it('keeps userEmail and fills missing userName from global default', () => {
      const out = mergeWithGlobalDefault(
        { userName: null, userEmail: 'a@b', userNameSource: 'none', userEmailSource: 'project' },
        { globalDefaultId: 'id-x', globalDefault: { userName: 'Bob', userEmail: 'b@x' } },
      );
      expect(out.userEmail).toBe('a@b');
      expect(out.userEmailSource).toBe('project');
      // userName was null → filled from global default
      expect(out.userName).toBe('Bob');
      expect(out.userNameSource).toBe('globalDefault');
    });

    it('returns repoConfig fully unchanged when both sides are filled', () => {
      const out = mergeWithGlobalDefault(
        { userName: 'Alice', userEmail: 'a@b', userNameSource: 'project', userEmailSource: 'project' },
        { globalDefaultId: 'id-x', globalDefault: { userName: 'Bob', userEmail: 'b@x' } },
      );
      expect(out.userName).toBe('Alice');
      expect(out.userEmail).toBe('a@b');
      expect(out.userNameSource).toBe('project');
      expect(out.userEmailSource).toBe('project');
    });
  });

  describe('fills missing sides from NiceSSH global default', () => {
    it('fills userName from global default when both null', () => {
      const out = mergeWithGlobalDefault(
        { userName: null, userEmail: null, userNameSource: 'none', userEmailSource: 'none' },
        { globalDefaultId: 'id-x', globalDefault: { userName: 'Bob', userEmail: 'b@x' } },
      );
      expect(out.userName).toBe('Bob');
      expect(out.userNameSource).toBe('globalDefault');
      expect(out.userEmail).toBe('b@x');
      expect(out.userEmailSource).toBe('globalDefault');
    });

    it('fills only the missing side (partial fill)', () => {
      const out = mergeWithGlobalDefault(
        { userName: null, userEmail: 'git@x', userNameSource: 'none', userEmailSource: 'global' },
        { globalDefaultId: 'id-x', globalDefault: { userName: 'Bob', userEmail: 'b@x' } },
      );
      // name missing → fill from global default
      expect(out.userName).toBe('Bob');
      expect(out.userNameSource).toBe('globalDefault');
      // email already from git global → keep, do NOT overwrite
      expect(out.userEmail).toBe('git@x');
      expect(out.userEmailSource).toBe('global');
    });
  });

  describe('no-op when global default is not set', () => {
    it('returns repoConfig unchanged when globalDefaultId is null', () => {
      const out = mergeWithGlobalDefault(
        { userName: null, userEmail: null, userNameSource: 'none', userEmailSource: 'none' },
        { globalDefaultId: null, globalDefault: null },
      );
      expect(out.userName).toBeNull();
      expect(out.userNameSource).toBe('none');
      expect(out.userEmail).toBeNull();
      expect(out.userEmailSource).toBe('none');
    });

    it('returns repoConfig unchanged when identity not found', () => {
      const out = mergeWithGlobalDefault(
        { userName: null, userEmail: null, userNameSource: 'none', userEmailSource: 'none' },
        { globalDefaultId: 'id-missing', globalDefault: null },
      );
      expect(out.userName).toBeNull();
    });
  });
});
