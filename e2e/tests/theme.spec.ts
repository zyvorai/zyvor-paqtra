// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { test, expect } from '@playwright/test';

test.describe('Theme', () => {
  test('starts in dark mode', async ({ page }) => {
    await page.goto('/');
    const html = page.locator('html');
    await expect(html).toHaveClass(/dark/);
  });

  test('toggle switches to light mode', async ({ page }) => {
    await page.goto('/');
    // Click Light Mode button in sidebar
    await page.click('button:has-text("Light Mode")');
    const html = page.locator('html');
    await expect(html).not.toHaveClass(/dark/);
  });

  test('theme persists after reload', async ({ page }) => {
    await page.goto('/');
    await page.click('button:has-text("Light Mode")');
    await page.reload();
    const html = page.locator('html');
    await expect(html).not.toHaveClass(/dark/);
    // Switch back
    await page.click('button:has-text("Dark Mode")');
  });
});
