// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test';

test.describe('Keyboard Shortcuts', () => {
  test('? opens help modal', async ({ page }) => {
    await page.goto('/');
    await page.keyboard.press('?');
    await expect(page.locator('text=Keyboard Shortcuts')).toBeVisible();
  });

  test('help modal shows shortcut categories', async ({ page }) => {
    await page.goto('/');
    await page.keyboard.press('?');
    await expect(page.locator('text=General')).toBeVisible();
    await expect(page.locator('text=Navigation')).toBeVisible();
    await expect(page.locator('text=Actions')).toBeVisible();
  });

  test('help modal closes on click outside', async ({ page }) => {
    await page.goto('/');
    await page.keyboard.press('?');
    await expect(page.locator('text=Keyboard Shortcuts')).toBeVisible();
    // Click the backdrop
    await page.click('.modal-backdrop', { position: { x: 10, y: 10 } });
    await expect(page.locator('text=Keyboard Shortcuts')).not.toBeVisible();
  });
});
