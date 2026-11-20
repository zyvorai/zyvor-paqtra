// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { isAxiosError } from 'axios';

/**
 * Open a fetched HTML report in a new tab. The report is fetched with the
 * session token and shown from a blob: URL, which runs on this app's origin, so
 * the report carries its own Content-Security-Policy and contains no script.
 */
export function openBlobInNewTab(blob: Blob): void {
  const url = URL.createObjectURL(new Blob([blob], { type: 'text/html' }));
  window.open(url, '_blank', 'noopener');
  // Give the new tab time to load it, then release the memory.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/**
 * Message for a failed blob request. With `responseType: 'blob'` an error body
 * such as `{"error": "Audit not found"}` arrives as a Blob, so it has to be read
 * before the usual `error` field can be found.
 */
export async function blobErrorMessage(err: unknown, fallback: string): Promise<string> {
  if (isAxiosError(err) && err.response?.data instanceof Blob) {
    try {
      const body = JSON.parse(await err.response.data.text()) as { error?: string; message?: string };
      return body.error ?? body.message ?? err.message;
    } catch {
      return err.message;
    }
  }
  return isAxiosError(err) ? err.message : fallback;
}
