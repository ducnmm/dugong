import type { Page } from '@playwright/test';

/**
 * Backend mocking helpers for E2E specs.
 *
 * The app talks to the API either with relative `/api/...` URLs (Home search)
 * or absolute `http://localhost:43001/api/...` URLs (api.ts helpers). The glob
 * patterns below (`**\/api/...`) match both, so specs never need a live backend.
 */

export const SAMPLE_ACCOUNT = {
  x_user_id: '555',
  x_handle: 'alice',
  sui_object_id: '0xobjectid000000000000000000000000000000000000000000000000000abcd',
  owner_address: '0xwallet0000000000000000000000000000000000000000000000000000beef',
};

export const SAMPLE_BALANCE = {
  balance_mist: 42_500_000_000,
  balance_sui: '42.5',
  balances: [
    {
      symbol: 'SUI',
      coin_type: '0x2::sui::SUI',
      balance_raw: 42_500_000_000,
      balance_formatted: '42.5',
      decimals: 9,
    },
  ],
  x_user_id: SAMPLE_ACCOUNT.x_user_id,
  sui_object_id: SAMPLE_ACCOUNT.sui_object_id,
};

export const SAMPLE_TRANSACTIONS = {
  data: [],
  total: 0,
  page: 1,
  limit: 5,
  total_pages: 0,
};

async function fulfillJson(route: import('@playwright/test').Route, body: unknown) {
  await route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify(body),
  });
}

/** Mock the account-search endpoint used by the Home page. */
export async function mockAccountSearch(page: Page, accounts = [SAMPLE_ACCOUNT]) {
  await page.route('**/api/accounts/search**', (route) =>
    fulfillJson(route, { accounts }),
  );
}

/** Mock the balance + transactions endpoints used by the Dashboard. */
export async function mockAccountData(page: Page) {
  await page.route('**/api/account/*/balance', (route) => fulfillJson(route, SAMPLE_BALANCE));
  await page.route('**/api/account/*/transactions**', (route) =>
    fulfillJson(route, SAMPLE_TRANSACTIONS),
  );
}

/** Mock the OAuth token-exchange endpoint hit by the callback flow. */
export async function mockTokenExchange(page: Page) {
  await page.route('**/api/auth/twitter/token', (route) =>
    fulfillJson(route, {
      user: { id: SAMPLE_ACCOUNT.x_user_id, username: SAMPLE_ACCOUNT.x_handle, name: 'Alice' },
      accessToken: 'test-access-token',
      dugongAccount: {
        sui_object_id: SAMPLE_ACCOUNT.sui_object_id,
        x_user_id: SAMPLE_ACCOUNT.x_user_id,
        x_handle: SAMPLE_ACCOUNT.x_handle,
        owner_address: SAMPLE_ACCOUNT.owner_address,
      },
    }),
  );
}

/** Seed localStorage so the app boots already authenticated as the sample user. */
export async function seedAuthenticatedUser(page: Page) {
  await page.addInitScript((account) => {
    localStorage.setItem(
      'dugong_user',
      JSON.stringify({
        twitterHandle: account.x_handle,
        twitterUserId: account.x_user_id,
        suiObjectId: account.sui_object_id,
        linkedWalletAddress: account.owner_address,
      }),
    );
    localStorage.setItem('dugong_access_token', 'test-access-token');
  }, SAMPLE_ACCOUNT);
}

/** Seed sessionStorage PKCE values so a callback with `state` is accepted. */
export async function seedPkce(page: Page, state: string, verifier = 'test-verifier') {
  await page.addInitScript(
    ([s, v]) => {
      sessionStorage.setItem('x_oauth_state', s);
      sessionStorage.setItem('x_oauth_code_verifier', v);
    },
    [state, verifier],
  );
}
