import { ipc } from './client';

export interface EnvCheck {
  tool: string;
  status: 'ok' | 'missing' | 'warning';
  detail: string;
}

export const checkEnvironment = () => ipc<EnvCheck[]>('check_environment');
export const clearHistory = () => ipc<void>('clear_history');
export const readLogTail = (lines: number) => ipc<string>('read_log_tail', { lines });
export const clearLog = () => ipc<void>('clear_log');

export interface SigningTestResult {
  ok: boolean;
  /// Single-letter `git log --format=%G?` code:
  /// G=good, U=good-unknown, Y=good-expired, B=bad,
  /// X=expired-key, R=revoked, E=cannot-check, N=none
  status: string;
  signerEmail: string | null;
  signerKeyFpr: string | null;
  message: string;
}

export const testSigningSetup = (
  identityId: string,
  signingKeyId: string,
  signingKeyKind: string,
) =>
  ipc<SigningTestResult>('test_signing_setup', {
    identityId,
    signingKeyId,
    signingKeyKind,
  });
