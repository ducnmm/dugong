import { describe, it, expect, beforeEach } from 'vitest';
import {
  generateCodeVerifier,
  generateCodeChallenge,
  generateState,
  storePKCE,
  retrievePKCE,
  clearPKCE,
} from './pkce';

describe('pkce', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  it('generates a base64url-safe code verifier with no padding or reserved chars', () => {
    const verifier = generateCodeVerifier();
    // 32 random bytes → 43 base64url chars (no padding).
    expect(verifier).toHaveLength(43);
    expect(verifier).toMatch(/^[A-Za-z0-9\-_]+$/);
  });

  it('generates unique verifiers across calls', () => {
    expect(generateCodeVerifier()).not.toBe(generateCodeVerifier());
  });

  it('derives a stable, url-safe SHA-256 challenge from a verifier', async () => {
    const verifier = 'test-verifier-value';
    const challenge = await generateCodeChallenge(verifier);
    const again = await generateCodeChallenge(verifier);

    expect(challenge).toBe(again); // deterministic
    expect(challenge).toMatch(/^[A-Za-z0-9\-_]+$/);
    expect(challenge).not.toContain('=');
  });

  it('produces different challenges for different verifiers', async () => {
    const a = await generateCodeChallenge('verifier-a');
    const b = await generateCodeChallenge('verifier-b');
    expect(a).not.toBe(b);
  });

  it('generates a state token', () => {
    const state = generateState();
    expect(state).toMatch(/^[A-Za-z0-9\-_]+$/);
    expect(state.length).toBeGreaterThan(0);
  });

  it('stores and retrieves PKCE values from sessionStorage', () => {
    storePKCE('verifier-123', 'state-abc');
    expect(retrievePKCE()).toEqual({
      codeVerifier: 'verifier-123',
      state: 'state-abc',
    });
  });

  it('returns nulls when nothing is stored', () => {
    expect(retrievePKCE()).toEqual({ codeVerifier: null, state: null });
  });

  it('clears stored PKCE values', () => {
    storePKCE('verifier-123', 'state-abc');
    clearPKCE();
    expect(retrievePKCE()).toEqual({ codeVerifier: null, state: null });
  });
});
