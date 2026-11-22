// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { test, expect } from '@playwright/test';

test.describe('Navigation', () => {
  test('loads dashboard by default', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('text=Dashboard')).toBeVisible();
  });

  test('sidebar navigation works', async ({ page }) => {
    await page.goto('/');

    // Navigate to Flows
    await page.click('nav >> text=Flows');
    await expect(page).toHaveURL('/flows');
    await expect(page.locator('h1')).toContainText('Flow Monitoring');

    // Navigate to Policies
    await page.click('nav >> text=Policies');
    await expect(page).toHaveURL('/policies');
    await expect(page.locator('h1')).toContainText('Policy Management');

    // Navigate to Anomalies
    await page.click('nav >> text=Anomalies');
    await expect(page).toHaveURL('/anomalies');
    await expect(page.locator('h1')).toContainText('Anomaly Detection');
  });

  test('sidebar sections are visible', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('text=Overview')).toBeVisible();
    await expect(page.locator('text=Observability')).toBeVisible();
    await expect(page.locator('text=Security')).toBeVisible();
    await expect(page.locator('text=Intelligence')).toBeVisible();
    await expect(page.locator('text=Operations')).toBeVisible();
  });

  test('404 page for unknown routes', async ({ page }) => {
    await page.goto('/nonexistent-page');
    await expect(page.locator('text=404')).toBeVisible();
    await expect(page.locator('text=Go to Dashboard')).toBeVisible();
  });

  test('404 go to dashboard button works', async ({ page }) => {
    await page.goto('/nonexistent-page');
    await page.click('text=Go to Dashboard');
    await expect(page).toHaveURL('/');
  });
});
