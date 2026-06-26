import { describe, it, expect, vi } from 'vitest';
import { render, screen, queryByRole } from '@testing-library/react';
import { IdentityFormDialog } from '../../../src/features/identityForm/IdentityFormDialog';
import type { Identity } from '../../../src/ipc/identities';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => `[t:${key}]`,
  }),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
}));

const baseIdentity: Identity = {
  id: 'id-1',
  label: 'id_work',
  userName: 'Alice',
  userEmail: 'a@x',
  keyPath: '/Users/x/.ssh/e9ab98-GitHub/',
  matchPath: null,
  hostAlias: 'github.com',
  gitHost: 'github.com',
};

describe('IdentityFormDialog — edit-mode lock', () => {
  it('disables the label and keyPath inputs when editing an existing identity', () => {
    const { container } = render(
      <IdentityFormDialog
        open
        onOpenChange={() => {}}
        initial={baseIdentity}
        onSubmit={async () => {}}
      />,
    );
    const labelInput = screen.getByLabelText(/\[t:identityForm\.label\]/) as HTMLInputElement;
    const keyPathInput = screen.getByLabelText(/\[t:identityForm\.keyPath\]/) as HTMLInputElement;
    expect(labelInput.disabled).toBe(true);
    expect(labelInput.readOnly).toBe(true);
    expect(keyPathInput.disabled).toBe(true);
    expect(keyPathInput.readOnly).toBe(true);
    // In edit mode the browse button is intentionally removed (not just
    // disabled) — there is no point re-prompting for a path that cannot
    // change.
    expect(queryByRole(container, { name: /\[t:identityForm\.browseKey\]/ })).toBeNull();
  });

  it('keeps label and keyPath editable when creating a new identity', () => {
    render(
      <IdentityFormDialog
        open
        onOpenChange={() => {}}
        defaultKeyPath="/Users/x/.ssh/e9ab98-GitHub/"
        defaultLabel="id_work"
        onSubmit={async () => {}}
      />,
    );
    const labelInput = screen.getByLabelText(/\[t:identityForm\.label\]/) as HTMLInputElement;
    const keyPathInput = screen.getByLabelText(/\[t:identityForm\.keyPath\]/) as HTMLInputElement;
    expect(labelInput.disabled).toBe(false);
    expect(keyPathInput.disabled).toBe(false);
    // Browse button is rendered + enabled in create mode.
    const browse = screen.getByRole('button', { name: /\[t:identityForm\.browseKey\]/ }) as HTMLButtonElement;
    expect(browse.hasAttribute('disabled')).toBe(false);
  });
});
