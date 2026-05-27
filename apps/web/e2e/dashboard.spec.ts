import { test, expect } from '@playwright/test';
import { seedAuthenticatedUser, mockAccountData, SAMPLE_ACCOUNT } from './fixtures';

test.describe('dashboard', () => {
  test('shows the authenticated user and their mocked balance', async ({ page }) => {
    await seedAuthenticatedUser(page);
    await mockAccountData(page);

    await page.goto('/dashboard');

    // Stays on the dashboard (not redirected home by the auth guard).
    await expect(page).toHaveURL(/\/dashboard$/);

    // The signed-in handle is shown.
    await expect(page.getByText(`@${SAMPLE_ACCOUNT.x_handle}`).first()).toBeVisible();

    // The mocked SUI balance is rendered.
    await expect(page.getByText('42.5').first()).toBeVisible();
  });

  test('redirects unauthenticated visitors to home', async ({ page }) => {
    // No seeded auth → the /dashboard guard sends the user back to /.
    await page.goto('/dashboard');
    await expect(page).toHaveURL(/\/$/);
    await expect(page.getByRole('heading', { name: /Your.*Social Wallet/i })).toBeVisible();
  });
});
