// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import App from '../App';

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      status: 401,
      ok: false,
      headers: { get: () => 'application/json' },
    }),
  );
});

describe('App', () => {
  it('renders without crashing', async () => {
    const { container } = render(<App />);
    await waitFor(() => expect(container.firstChild).toBeTruthy());
  });

  it('shows the login page when there is no session', async () => {
    render(<App />);
    await waitFor(() => {
      expect(document.body.querySelector('.login-shell')).toBeTruthy();
    });
    expect(screen.getByRole('button', { name: /Sign in/i })).toBeTruthy();
    expect(screen.getByText(/Lab default/)).toBeTruthy();
    expect(screen.getByText(/Admin@321/)).toBeTruthy();
  });
});
