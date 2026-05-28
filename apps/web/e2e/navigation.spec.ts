import { test, expect } from '@playwright/test';
import { mockAccountSearch, SAMPLE_ACCOUNT } from './fixtures';

test.describe('navigation', () => {
  test('home page renders the hero and feature sections', async ({ page }) => {
    await page.goto('/');

    await expect(page.getByRole('heading', { name: /Your.*Social Wallet/i })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'How It Works' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Search Accounts' })).toBeVisible();
  });

  test('searching surfaces a result and navigates to the account view', async ({ page }) => {
    await mockAccountSearch(page);
    await page.goto('/');

    await page.getByPlaceholder(/Search by @handle/i).fill('alice');
    await page.getByRole('button', { name: 'Search', exact: true }).click();

    // The results panel renders with the mocked account.
    await expect(page.getByRole('heading', { name: /Search Results/i })).toBeVisible();
    await expect(
      page.locator('#search-section').getByText(`@${SAMPLE_ACCOUNT.x_handle}`),
    ).toBeVisible();

    await page.getByRole('button', { name: /View Account/i }).click();
    await expect(page).toHaveURL(new RegExp(`/account/${SAMPLE_ACCOUNT.x_user_id}$`));
  });

  test('unknown routes redirect home', async ({ page }) => {
    await page.goto('/this-route-does-not-exist');
    await expect(page).toHaveURL(/\/$/);
    await expect(page.getByRole('heading', { name: /Your.*Social Wallet/i })).toBeVisible();
  });
});
