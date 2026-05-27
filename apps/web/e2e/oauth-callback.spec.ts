import { test, expect } from '@playwright/test';
import { mockTokenExchange, mockAccountData, seedPkce } from './fixtures';

test.describe('oauth callback', () => {
  test('exchanges the code and lands on the dashboard', async ({ page }) => {
    const state = 'csrf-state-123';
    await seedPkce(page, state);
    await mockTokenExchange(page);
    await mockAccountData(page); // dashboard data after the post-login redirect

    await page.goto(`/callback?code=auth-code-abc&state=${state}`);

    // The callback confirms success, then redirects to /dashboard after a delay.
    await expect(page.getByRole('heading', { name: /Successfully Signed In/i })).toBeVisible();
    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(page.getByText('@alice').first()).toBeVisible();
  });

  test('shows an error when the state does not match (CSRF guard)', async ({ page }) => {
    await seedPkce(page, 'expected-state');
    await mockTokenExchange(page);

    await page.goto('/callback?code=auth-code-abc&state=attacker-state');

    await expect(page.getByRole('heading', { name: /Sign In Failed/i })).toBeVisible();
    await expect(page.getByText(/Invalid state parameter/i)).toBeVisible();
  });

  test('shows an error when OAuth params are missing', async ({ page }) => {
    await page.goto('/callback');
    await expect(page.getByRole('heading', { name: /Sign In Failed/i })).toBeVisible();
    await expect(page.getByText(/Missing authorization code or state/i)).toBeVisible();
  });
});
