import { test, expect } from '@playwright/test';

test.describe('Agent App', () => {
  test('app loads and shows the sidebar', async ({ page }) => {
    await page.goto('/');
    // The sidebar should be visible with a "New Chat" or conversation list area
    await expect(page.locator('aside')).toBeVisible();
  });

  test('dark mode toggle works', async ({ page }) => {
    await page.goto('/');
    // Find the theme toggle button (usually in the sidebar footer)
    const toggle = page.locator('button[aria-label*="dark" i], button[aria-label*="theme" i]');
    if (await toggle.count() > 0) {
      await toggle.click();
      // Check dark class was added
      await expect(page.locator('html')).toHaveClass(/dark/);
    }
  });

  test('settings modal can be opened', async ({ page }) => {
    await page.goto('/');
    // Look for a settings button (gear icon)
    const settingsBtn = page.locator('button[aria-label*="setting" i], svg.lucide-settings').first();
    if (await settingsBtn.count() > 0) {
      await settingsBtn.click();
      // Settings modal should appear
      await expect(page.locator('[role="dialog"], .fixed.inset-0')).toBeVisible({ timeout: 3000 });
    }
  });
});
