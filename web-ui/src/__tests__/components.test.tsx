// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import LoadingSpinner from '../components/LoadingSpinner';

describe('LoadingSpinner', () => {
  it('renders spinner', () => {
    const { container } = render(<LoadingSpinner />);
    expect(container.querySelector('.animate-spin')).toBeInTheDocument();
  });

  it('renders with text', () => {
    render(<LoadingSpinner text="Loading data..." />);
    expect(screen.getByText('Loading data...')).toBeInTheDocument();
  });

  it('renders small size', () => {
    const { container } = render(<LoadingSpinner size="sm" />);
    expect(container.querySelector('.w-4')).toBeInTheDocument();
  });

  it('renders large size', () => {
    const { container } = render(<LoadingSpinner size="lg" />);
    expect(container.querySelector('.w-12')).toBeInTheDocument();
  });
});
