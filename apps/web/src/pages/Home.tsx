import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { ExternalLink, Search } from 'lucide-react';
import { useAuth } from '../contexts/useAuth';
import { useXAuth } from '../hooks/useXAuth';
import { useDocumentTitle } from '../hooks/useDocumentTitle';
import { API_BASE_URL } from '../utils/constants';

const HOME_BACKGROUND_SRC = '/dugong-home-background-new.png';
const HOME_MASCOT_SRC = '/dugong-home-mascot.png';
const HOME_DOCS_URL = import.meta.env.VITE_DOCS_URL || 'http://127.0.0.1:3004';
const X_DEFAULT_AVATAR_SRC = 'https://abs.twimg.com/sticky/default_profile_images/default_profile_400x400.png';
const HOME_COMMAND_COLORS = ['bg-yellow-100', 'bg-cyan-100', 'bg-lime-100'] as const;
const HOME_COMMANDS = [
  '@DugongWallet create account',
  '@DugongWallet send 0.01 DUG to @alice',
  '@DugongWallet create market: Will BTC reach 120k?',
  '@DugongWallet predict 1 DUG on yes',
  '@DugongWallet reward top 3 replies to this tweet with 5 DUG each',
  '@DugongWallet claim',
].map((text, i) => ({ text, color: HOME_COMMAND_COLORS[i % HOME_COMMAND_COLORS.length] }));

const getTweetIntentUrl = (text: string) =>
  `https://x.com/intent/tweet?text=${encodeURIComponent(text)}`;

const getXAvatarUrl = (handle: string) =>
  `https://unavatar.io/x/${encodeURIComponent(handle.trim().replace(/^@/, ''))}`;

interface AccountSearchResult {
  x_user_id: string;
  x_handle: string;
  sui_object_id: string;
  owner_address?: string;
  profile_image_url?: string | null;
}

export const Home: React.FC = () => {
  useDocumentTitle('Dugong Account Search');
  const navigate = useNavigate();
  const { isAuthenticated } = useAuth();
  const { initiateLogin: initiateXLogin, isLoading: xAuthLoading } = useXAuth();

  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<AccountSearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [searchError, setSearchError] = useState<string>('');
  const [hasSearched, setHasSearched] = useState(false);
  const [isSearchFocused, setIsSearchFocused] = useState(false);
  const hasSearchResults = searchResults.length > 0;
  const shouldShowCommandRail = !hasSearchResults && !hasSearched;

  const handleSearch = async (e: React.FormEvent) => {
    e.preventDefault();

    const query = searchQuery.trim();

    if (!query || isSearching) {
      return;
    }

    setIsSearching(true);
    setSearchError('');
    setSearchResults([]);
    setHasSearched(true);

    try {
      const response = await fetch(`${API_BASE_URL}/api/accounts/search?q=${encodeURIComponent(query)}`);

      if (!response.ok) {
        throw new Error('Search failed');
      }

      const data = await response.json();
      setSearchResults(data.accounts || []);
    } catch (err) {
      console.error('Search error:', err);
      setSearchError('Failed to search accounts. Please try again.');
    } finally {
      setIsSearching(false);
    }
  };

  const handleDashboardClick = () => {
    if (isAuthenticated) {
      navigate('/dashboard');
      return;
    }

    void initiateXLogin();
  };

  return (
    <main className="h-screen overflow-hidden bg-white p-2 dark:bg-black sm:p-4">
      <div
        className="relative h-full overflow-hidden rounded-[2rem] bg-cover bg-center bg-no-repeat sm:rounded-[2.5rem]"
        style={{
          backgroundImage: `url(${HOME_BACKGROUND_SRC})`,
          backgroundPosition: 'center bottom',
        }}
      >
        <div className="absolute right-4 top-4 z-20 sm:right-6 sm:top-6">
          <button
            onClick={handleDashboardClick}
            disabled={xAuthLoading}
            className="btn-sui home-dashboard-button disabled:pointer-events-none disabled:opacity-50"
          >
            Dashboard
          </button>
        </div>

        <section
          className={`mx-auto flex h-full min-h-0 w-full max-w-3xl flex-col overflow-y-auto px-4 pt-6 sm:px-6 sm:pt-8 lg:px-8 ${
            hasSearchResults
              ? 'justify-start pb-6 sm:pb-8'
              : 'justify-center pb-40 sm:pb-44'
          }`}
        >
          <div className="pointer-events-none relative z-10 mb-[-68px] flex justify-center sm:mb-[-84px]">
            <img
              src={HOME_MASCOT_SRC}
              alt=""
              aria-hidden="true"
              className="w-full max-w-[430px] select-none"
              draggable={false}
            />
          </div>

          <form
            onSubmit={handleSearch}
            className="relative z-20 w-full"
            aria-label="Search Dugong accounts"
          >
            <div className="account-search-field relative">
              <label htmlFor="account-search" className="sr-only">
                Search account
              </label>
              <Search className="account-search-icon pointer-events-none absolute left-5 top-1/2 z-10 h-5 w-5 text-black" />
              <input
                id="account-search"
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onFocus={() => setIsSearchFocused(true)}
                onBlur={() => setIsSearchFocused(false)}
                placeholder="Search by @handle, user ID, or 0x... address"
                className="account-search-input h-16 rounded-lg pl-14 pr-12 text-base sm:text-lg"
                autoComplete="off"
              />
              {isSearching && (
                <span
                  className="pointer-events-none absolute right-5 top-1/2 h-5 w-5 -translate-y-1/2 animate-spin rounded-full border-4 border-black border-t-cyan-300 bg-white"
                  aria-hidden="true"
                />
              )}
            </div>
          </form>

          {!hasSearchResults && (
            <a
              href={HOME_DOCS_URL}
              target="_blank"
              rel="noreferrer"
              className={`home-docs-link mt-6 ${isSearchFocused ? 'home-docs-link-hidden' : ''}`}
              aria-label="Open Dugong documentation"
              aria-hidden={isSearchFocused}
              tabIndex={isSearchFocused ? -1 : undefined}
            >
              <span>Docs</span>
              <ExternalLink className="h-5 w-5 shrink-0 text-black" aria-hidden="true" />
            </a>
          )}

          {searchError && (
            <div className="mt-6 rounded-lg border-2 border-black bg-red-200 p-4 shadow-neo-md">
              <p className="font-bold text-black">{searchError}</p>
            </div>
          )}

          {hasSearchResults && (
            <div className="glass relative z-20 mt-6 flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg">
              <div className="shrink-0 border-b-2 border-black bg-yellow-200 p-5">
                <h2 className="hero-font text-3xl font-black text-black">
                  Search Results ({searchResults.length})
                </h2>
              </div>
              <div className="min-h-0 flex-1 divide-y-2 divide-black overflow-y-auto overscroll-contain">
                {searchResults.map((account) => (
                  <div
                    key={account.x_user_id}
                    className="flex flex-col gap-5 p-5 transition-colors hover:bg-cyan-200 sm:flex-row sm:items-center sm:justify-between"
                  >
                    <div className="flex min-w-0 flex-1 items-center gap-3">
                      <img
                        src={account.profile_image_url || getXAvatarUrl(account.x_handle)}
                        alt={`@${account.x_handle}`}
                        className="h-11 w-11 shrink-0 rounded-md border border-black bg-white object-cover shadow-neo-sm"
                        referrerPolicy="no-referrer"
                        onError={(event) => {
                          event.currentTarget.onerror = null;
                          event.currentTarget.src = X_DEFAULT_AVATAR_SRC;
                        }}
                      />
                      <div className="min-w-0">
                        <p className="truncate text-xl font-black text-black">
                          @{account.x_handle}
                        </p>
                      </div>
                    </div>

                    <button
                      onClick={() => navigate(`/account/${account.x_user_id}/dashboard`)}
                      className="btn-glass flex h-11 items-center justify-center gap-2 text-sm sm:w-auto"
                    >
                      View Account
                      <ExternalLink className="h-3.5 w-3.5" />
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}

          {!isSearching && searchResults.length === 0 && hasSearched && !searchError && (
            <div className="glass mt-6 rounded-lg bg-white py-10 text-center">
              <Search className="mx-auto mb-3 h-7 w-7 text-black" />
              <p className="px-4 font-bold text-black">
                No accounts found for <span>"{searchQuery}"</span>
              </p>
            </div>
          )}
        </section>

        {shouldShowCommandRail && (
          <div
            className={`absolute inset-x-0 bottom-8 z-20 sm:bottom-12 ${isSearchFocused ? 'pointer-events-none invisible' : ''}`}
            aria-hidden={isSearchFocused}
          >
            <div className="overflow-hidden py-3">
              <div className="home-command-marquee flex w-max gap-3">
                {[...HOME_COMMANDS, ...HOME_COMMANDS].map((command, index) => (
                  <div
                    key={`${command.text}-${index}`}
                    className={`flex min-w-max items-center gap-4 rounded-lg border-2 border-black px-7 py-5 text-black shadow-neo-sm ${command.color}`}
                  >
                    <code className="font-mono text-sm font-black leading-none sm:text-base">
                      {command.text}
                    </code>
                    <a
                      href={getTweetIntentUrl(command.text)}
                      target="_blank"
                      rel="noreferrer"
                      className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border-2 border-black bg-white text-black shadow-neo-sm transition-[transform,background-color,box-shadow] hover:-translate-x-px hover:-translate-y-px hover:bg-cyan-300 hover:shadow-neo-md"
                      aria-label={`Compose on X: ${command.text}`}
                    >
                      <ExternalLink className="h-5 w-5" aria-hidden="true" />
                    </a>
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}
      </div>
    </main>
  );
};
