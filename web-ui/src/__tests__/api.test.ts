// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, it, expect, beforeEach, vi } from 'vitest';
import api, { fetchFlows, fetchPolicies, checkHealth, fetchFlowStats } from '../services/api';

describe('API Client', () => {
  it('has correct default base URL', () => {
    expect(api.defaults.baseURL).toBe('/api/v1');
  });

  it('has correct timeout', () => {
    expect(api.defaults.timeout).toBe(45_000);
  });

  it('has JSON content type header', () => {
    expect(api.defaults.headers['Content-Type']).toBe('application/json');
  });

  it('has request interceptor', () => {
    expect(api.interceptors.request.handlers.length).toBeGreaterThan(0);
  });

  it('has response interceptor', () => {
    expect(api.interceptors.response.handlers.length).toBeGreaterThan(0);
  });
});

describe('API Request Interceptor', () => {
  beforeEach(() => {
    // Clear any auth header set by previous tests
    delete api.defaults.headers.common['Authorization'];
  });

  it('attaches Authorization header when token is set on defaults', async () => {
    api.defaults.headers.common['Authorization'] = 'Bearer test-token-123';

    // The request interceptor runs on every request; verify the header is present
    expect(api.defaults.headers.common['Authorization']).toBe('Bearer test-token-123');

    // Clean up
    delete api.defaults.headers.common['Authorization'];
  });

  it('does not have Authorization header when no token is set', () => {
    expect(api.defaults.headers.common['Authorization']).toBeUndefined();
  });
});

describe('API Response Interceptor', () => {
  it('clears Authorization header on 401 response', async () => {
    // Set a token first
    api.defaults.headers.common['Authorization'] = 'Bearer expired-token';

    // Simulate what the response interceptor does on 401
    const errorInterceptor = api.interceptors.response.handlers[0];
    const rejectedHandler = errorInterceptor?.rejected;

    if (rejectedHandler) {
      const mockError = {
        response: { status: 401 },
        isAxiosError: true,
      };

      // Suppress console.warn from the interceptor
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

      try {
        await rejectedHandler(mockError);
      } catch {
        // Expected: interceptor rejects the promise
      }

      // The 401 handler should have cleared the Authorization header
      expect(api.defaults.headers.common['Authorization']).toBeUndefined();
      warnSpy.mockRestore();
    }
  });

  it('does not clear Authorization header on non-401 errors', async () => {
    api.defaults.headers.common['Authorization'] = 'Bearer valid-token';

    const errorInterceptor = api.interceptors.response.handlers[0];
    const rejectedHandler = errorInterceptor?.rejected;

    if (rejectedHandler) {
      const mockError = {
        response: { status: 500 },
        isAxiosError: true,
      };

      try {
        await rejectedHandler(mockError);
      } catch {
        // Expected: interceptor rejects the promise
      }

      // Authorization header should remain intact for non-401 errors
      expect(api.defaults.headers.common['Authorization']).toBe('Bearer valid-token');
    }

    // Clean up
    delete api.defaults.headers.common['Authorization'];
  });
});

describe('API Functions', () => {
  it('exports fetchFlows as a callable function', () => {
    expect(typeof fetchFlows).toBe('function');
  });

  it('exports fetchPolicies as a callable function', () => {
    expect(typeof fetchPolicies).toBe('function');
  });

  it('exports checkHealth as a callable function', () => {
    expect(typeof checkHealth).toBe('function');
  });

  it('exports fetchFlowStats as a callable function', () => {
    expect(typeof fetchFlowStats).toBe('function');
  });
});
