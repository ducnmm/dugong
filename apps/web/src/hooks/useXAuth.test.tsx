import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useXAuth } from './useXAuth';
import { storePKCE } from '../utils/pkce';

function mockResponse(body: unknown, init?: { status?: number; ok?: boolean }) {
  const status = init?.status ?? 200;
  return {
    ok: init?.ok ?? (status >= 200 && status < 300),
    status,
    json: async () => body,
  } as Response;
}

describe('useXAuth', () => {
  beforeEach(() => {
    sessionStorage.clear();
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('starts idle with no error', () => {
    const { result } = renderHook(() => useXAuth());
    expect(result.current.isLoading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('exchanges the code for a token when state matches', async () => {
    // Simulate a prior initiateLogin having stored these PKCE values.
    storePKCE('verifier-xyz', 'state-123');

    const authResult = {
      user: { id: '555', username: 'alice', name: 'Alice' },
      accessToken: 'token-abc',
    };
    vi.mocked(fetch).mockResolvedValue(mockResponse(authResult));

    const { result } = renderHook(() => useXAuth());

    let returned;
    await act(async () => {
      returned = await result.current.handleCallback('auth-code', 'state-123');
    });

    expect(returned).toEqual(authResult);
    expect(result.current.error).toBeNull();
    expect(result.current.isLoading).toBe(false);

    // PKCE values must be cleared after a successful exchange.
    expect(sessionStorage.getItem('x_oauth_code_verifier')).toBeNull();

    const [url, opts] = vi.mocked(fetch).mock.calls[0];
    expect(String(url)).toContain('/api/auth/twitter/token');
    const body = JSON.parse((opts as RequestInit).body as string);
    expect(body).toMatchObject({ code: 'auth-code', code_verifier: 'verifier-xyz' });
  });

  it('rejects a callback whose state does not match (CSRF guard)', async () => {
    storePKCE('verifier-xyz', 'state-123');
    const { result } = renderHook(() => useXAuth());

    await act(async () => {
      await expect(
        result.current.handleCallback('auth-code', 'attacker-state'),
      ).rejects.toThrow('Invalid state parameter');
    });

    expect(result.current.error).toContain('Invalid state parameter');
    expect(fetch).not.toHaveBeenCalled();
  });

  it('surfaces a backend error on a failed token exchange', async () => {
    storePKCE('verifier-xyz', 'state-123');
    vi.mocked(fetch).mockResolvedValue(
      mockResponse({ error: 'token exchange rejected' }, { status: 400 }),
    );

    const { result } = renderHook(() => useXAuth());

    await act(async () => {
      await expect(
        result.current.handleCallback('auth-code', 'state-123'),
      ).rejects.toThrow('token exchange rejected');
    });

    expect(result.current.error).toBe('token exchange rejected');
    // PKCE values are cleared even on failure.
    expect(sessionStorage.getItem('x_oauth_code_verifier')).toBeNull();
  });
});
