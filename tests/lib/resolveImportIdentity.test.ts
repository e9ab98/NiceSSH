import { describe, it, expect } from 'vitest';
import {
  resolveImportIdentity,
  type ImportIdentityInput,
} from '../../src/lib/resolveImportIdentity';

const baseInput: ImportIdentityInput = {
  repoConfig: null,
  includeIfResult: { resolved: false },
  globalDefault: null,
};

describe('resolveImportIdentity', () => {
  describe('priority 1: project-level config wins', () => {
    it('returns source=project when repoConfig.userName is non-empty', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        repoConfig: { userName: 'Alice', userEmail: null, sshCommand: null },
        globalDefault: { identityId: 'id-global', exists: true },
      });
      expect(out).toEqual({
        source: 'project',
        identityId: null,
        needsBinding: false,
      });
    });

    it('returns source=project when repoConfig.userEmail is non-empty', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        repoConfig: { userName: null, userEmail: 'a@b', sshCommand: null },
      });
      expect(out.source).toBe('project');
      expect(out.needsBinding).toBe(false);
    });

    it('returns source=project when repoConfig.sshCommand is non-empty', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        repoConfig: { userName: null, userEmail: null, sshCommand: 'ssh -i /tmp/k' },
      });
      expect(out.source).toBe('project');
      expect(out.needsBinding).toBe(false);
    });

    it('treats empty strings as "not configured" and falls through', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        repoConfig: { userName: '', userEmail: '', sshCommand: '' },
        globalDefault: { identityId: 'id-global', exists: true },
      });
      // Falls through to globalDefault.
      expect(out.source).toBe('globalDefault');
      expect(out.needsBinding).toBe(true);
      expect(out.identityId).toBe('id-global');
    });

    it('project wins over includeIf and globalDefault', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        repoConfig: { userName: 'Alice', userEmail: null, sshCommand: null },
        includeIfResult: { resolved: true, identityId: 'id-inc' },
        globalDefault: { identityId: 'id-global', exists: true },
      });
      expect(out.source).toBe('project');
      expect(out.needsBinding).toBe(false);
    });
  });

  describe('priority 2: includeIf hit', () => {
    it('returns source=includeIf when includeIfResult.resolved=true', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        includeIfResult: { resolved: true, identityId: 'id-inc' },
        globalDefault: { identityId: 'id-global', exists: true },
      });
      expect(out).toEqual({
        source: 'includeIf',
        identityId: 'id-inc',
        needsBinding: false,
      });
    });

    it('falls through when includeIfResult.resolved=false (current "unimplemented" placeholder)', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        includeIfResult: { resolved: false },
        globalDefault: { identityId: 'id-global', exists: true },
      });
      expect(out.source).toBe('globalDefault');
      expect(out.identityId).toBe('id-global');
      expect(out.needsBinding).toBe(true);
    });

    it('includeIf wins over globalDefault', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        includeIfResult: { resolved: true, identityId: 'id-inc' },
        globalDefault: { identityId: 'id-global', exists: true },
      });
      expect(out.source).toBe('includeIf');
      expect(out.needsBinding).toBe(false);
    });
  });

  describe('priority 3: global default fallback', () => {
    it('returns source=globalDefault when only globalDefault exists', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        globalDefault: { identityId: 'id-global', exists: true },
      });
      expect(out).toEqual({
        source: 'globalDefault',
        identityId: 'id-global',
        needsBinding: true,
      });
    });

    it('returns source=none when globalDefault points to a deleted identity', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        globalDefault: { identityId: 'id-deleted', exists: false },
      });
      expect(out).toEqual({
        source: 'none',
        identityId: null,
        needsBinding: false,
      });
    });

    it('returns source=none when globalDefault.identityId is null (unset)', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        globalDefault: { identityId: null, exists: false },
      });
      expect(out.source).toBe('none');
      expect(out.needsBinding).toBe(false);
    });

    it('returns source=none when globalDefault itself is null', () => {
      const out = resolveImportIdentity({
        ...baseInput,
        globalDefault: null,
      });
      expect(out).toEqual({
        source: 'none',
        identityId: null,
        needsBinding: false,
      });
    });
  });

  describe('priority 4: nothing', () => {
    it('returns source=none when all inputs are empty', () => {
      const out = resolveImportIdentity(baseInput);
      expect(out).toEqual({
        source: 'none',
        identityId: null,
        needsBinding: false,
      });
    });
  });

  describe('regression: does not depend on SSH key presence', () => {
    it('returns globalDefault even when globalDefault identity has no SSH key info available', () => {
      // The pre-spec code path required globalGit.sshKeyPath to be non-null
      // before it would even consider the global default. The new resolver
      // must work purely off the cfg pointer, not the parsed key path.
      const out = resolveImportIdentity({
        ...baseInput,
        globalDefault: { identityId: 'id-no-key', exists: true },
      });
      expect(out.source).toBe('globalDefault');
      expect(out.identityId).toBe('id-no-key');
      expect(out.needsBinding).toBe(true);
    });
  });
});
