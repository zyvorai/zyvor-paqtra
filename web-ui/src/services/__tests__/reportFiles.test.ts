// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { describe, it, expect, vi, afterEach } from 'vitest';
import { AxiosError, AxiosResponse } from 'axios';
import { openBlobInNewTab, downloadBlob, blobErrorMessage } from '../reportFiles';

const blobResponse = (status: number, body: string) =>
  ({ status, data: new Blob([body], { type: 'application/json' }) }) as AxiosResponse;
const fail = (status: number, body: string) =>
  new AxiosError(`Request failed with status code ${status}`, String(status), undefined, undefined, blobResponse(status, body));

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe('blobErrorMessage', () => {
  it("reads the API's error field out of a Blob body", async () => {
    expect(await blobErrorMessage(fail(404, '{"error":"Audit not found"}'), 'fallback')).toBe('Audit not found');
  });

  it('falls back to message, then to the status text, for other bodies', async () => {
    expect(await blobErrorMessage(fail(500, '{"message":"boom"}'), 'fallback')).toBe('boom');
    expect(await blobErrorMessage(fail(502, 'not json'), 'fallback')).toBe('Request failed with status code 502');
    expect(await blobErrorMessage(fail(500, '{}'), 'fallback')).toBe('Request failed with status code 500');
  });

  it('handles ordinary JSON errors and non-axios errors', async () => {
    const plain = new AxiosError('Network Error');
    expect(await blobErrorMessage(plain, 'fallback')).toBe('Network Error');
    expect(await blobErrorMessage(new Error('x'), 'fallback')).toBe('fallback');
    expect(await blobErrorMessage('weird', 'fallback')).toBe('fallback');
  });
});

describe('openBlobInNewTab', () => {
  it('opens an HTML blob URL without giving the tab access to this window, then releases it', () => {
    vi.useFakeTimers();
    const create = vi.fn(() => 'blob:report');
    const revoke = vi.fn();
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke }));
    const open = vi.spyOn(window, 'open').mockReturnValue(null);

    openBlobInNewTab(new Blob(['<p>x</p>'], { type: 'application/octet-stream' }));

    const shown = (create.mock.calls[0] as unknown as [Blob])[0];
    expect(shown.type).toBe('text/html');
    expect(open).toHaveBeenCalledWith('blob:report', '_blank', 'noopener');
    expect(revoke).not.toHaveBeenCalled();
    vi.advanceTimersByTime(60_000);
    expect(revoke).toHaveBeenCalledWith('blob:report');
  });
});

describe('downloadBlob', () => {
  it('clicks a temporary link carrying the file name, and cleans up', () => {
    vi.useFakeTimers();
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: vi.fn(() => 'blob:file'), revokeObjectURL: vi.fn() }));
    const make = document.createElement.bind(document);
    let link: { download: string; href: string; click: () => void } | null = null;
    vi.spyOn(document, 'createElement').mockImplementation((tag: string) => {
      const el = make(tag);
      if (tag === 'a') {
        el.click = vi.fn();
        link = el as unknown as typeof link;
      }
      return el;
    });

    downloadBlob(new Blob(['a,b']), 'paqtra-checks-hipaa-1234abcd.csv');

    expect(link).not.toBeNull();
    expect(link!.download).toBe('paqtra-checks-hipaa-1234abcd.csv');
    expect(link!.href).toBe('blob:file');
    expect(link!.click).toHaveBeenCalledTimes(1);
    expect(document.body.querySelector('a[download]')).toBeNull();
    vi.runAllTimers();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:file');
  });
});
