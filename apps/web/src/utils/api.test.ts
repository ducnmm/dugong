import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getAccountByWallet,
  getAccountBalance,
  createSponsoredTransaction,
  mistToSui,
  shortenDigest,
  getExplorerUrl,
  formatTimestamp,
} from './api';

// Build a minimal Response-like stub for the global fetch mock.
function mockResponse(body: unknown, init?: { status?: number; ok?: boolean }) {
  const status = init?.status ?? 200;
  return {
    ok: init?.ok ?? (status >= 200 && status < 300),
    status,
    json: async () => body,
  } as Response;
}

describe('api pure helpers', () => {
  it('converts MIST to SUI and trims trailing zeros', () => {
    expect(mistToSui(1_000_000_000)).toBe('1');
    expect(mistToSui(1_500_000_000)).toBe('1.5');
    expect(mistToSui('2500000000')).toBe('2.5');
    expect(mistToSui(0)).toBe('0');
  });

  it('shortens long digests and leaves short ones intact', () => {
    expect(shortenDigest('0x1234567890abcdef')).toBe('0x1234...abcdef');
    expect(shortenDigest('short')).toBe('short');
  });

  it('builds the explorer URL for the given network', () => {
    expect(getExplorerUrl('0xabc')).toBe('https://suiscan.xyz/testnet/tx/0xabc');
    expect(getExplorerUrl('0xabc', 'mainnet')).toBe('https://suiscan.xyz/mainnet/tx/0xabc');
  });

  it('formats a falsy timestamp as Unknown', () => {
    expect(formatTimestamp(0)).toBe('Unknown');
  });
});

describe('api fetch functions', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('returns the account on a 200 response', async () => {
    const account = {
      x_user_id: '555',
      x_handle: 'alice',
      sui_object_id: '0xobj',
      owner_address: '0xwallet',
    };
    vi.mocked(fetch).mockResolvedValue(mockResponse(account));

    await expect(getAccountByWallet('0xwallet')).resolves.toEqual(account);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/account/by-wallet/0xwallet'),
    );
  });

  it('returns null when the account is not found (404)', async () => {
    vi.mocked(fetch).mockResolvedValue(mockResponse(null, { status: 404 }));
    await expect(getAccountByWallet('0xmissing')).resolves.toBeNull();
  });

  it('throws on a non-404 error status', async () => {
    vi.mocked(fetch).mockResolvedValue(mockResponse(null, { status: 500 }));
    await expect(getAccountByWallet('0xboom')).rejects.toThrow('HTTP error! status: 500');
  });

  it('throws on a failed balance fetch', async () => {
    vi.mocked(fetch).mockResolvedValue(mockResponse(null, { status: 500 }));
    await expect(getAccountBalance('0xobj')).rejects.toThrow('HTTP error! status: 500');
  });

  it('surfaces the backend error message when sponsoring fails', async () => {
    vi.mocked(fetch).mockResolvedValue(
      mockResponse({ error: 'enclave unavailable' }, { status: 502 }),
    );

    await expect(
      createSponsoredTransaction({
        network: 'testnet',
        txBytes: 'abc',
        sender: '0xsender',
      }),
    ).rejects.toThrow('enclave unavailable');
  });
});
