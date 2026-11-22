// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';

interface LoadingSpinnerProps {
  size?: 'sm' | 'md' | 'lg';
  text?: string;
  fullScreen?: boolean;
}

const SIZE_CLASS = { sm: 'w-4', md: 'w-8', lg: 'w-12' } as const;

const LoadingSpinner: React.FC<LoadingSpinnerProps> = ({ size = 'md', text, fullScreen }) => {
  const content = (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
      <div className={`paqtra-spinner animate-spin ${SIZE_CLASS[size]}`} aria-hidden />
      {text ? <span className="eyebrow">{text}</span> : null}
    </div>
  );

  if (fullScreen) {
    return (
      <div className="login-shell" style={{ justifyContent: 'center' }}>
        {content}
      </div>
    );
  }

  return <div style={{ display: 'flex', justifyContent: 'center', padding: '3rem 0' }}>{content}</div>;
};

export default LoadingSpinner;
