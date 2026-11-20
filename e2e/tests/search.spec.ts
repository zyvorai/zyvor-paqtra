// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test';

test.describe('Global Search', () => {
  test('opens with / key', async ({ page }) => {
    await page.goto('/');
    await page.keyboard.press('/');
    await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
  });

  test('opens with search button click', async ({ page }) => {
    await page.goto('/');
    await page.click('button:has-text("Search")');
    await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
  });

  test('filters results by query', async ({ page }) => {
    await page.goto('/');
    await page.keyboard.press('/');
    await page.fill('input[placeholder*="Search"]', 'policy');
    // Should show policy-related results
    await expect(page.locator('text=Policy Management')).toBeVisible();
  });

  test('closes with Escape', async ({ page }) => {
    await page.goto('/');
    await page.keyboard.press('/');
    await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('input[placeholder*="Search"]')).not.toBeVisible();
  });

  test('navigates on Enter', async ({ page }) => {
    await page.goto('/');
    await page.keyboard.press('/');
    await page.fill('input[placeholder*="Search"]', 'Flows');
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL('/flows');
  });
});
