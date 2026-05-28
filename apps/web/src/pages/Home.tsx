import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { ExternalLink, Search } from 'lucide-react';
import { useAuth } from '../contexts/useAuth';
import { useXAuth } from '../hooks/useXAuth';
import { useDocumentTitle } from '../hooks/useDocumentTitle';

const HOME_BACKGROUND_SRC = '/dugong-home-background-new.png';
const HOME_MASCOT_SRC = '/dugong-home-mascot.png';
const X_DEFAULT_AVATAR_SRC = 'https://abs.twimg.com/sticky/default_profile_images/default_profile_400x400.png';

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
      const response = await fetch(`/api/accounts/search?q=${encodeURIComponent(query)}`);

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

        <section className="mx-auto flex h-full w-full max-w-3xl flex-col justify-center px-4 pb-40 pt-6 sm:px-6 sm:pb-44 sm:pt-8 lg:px-8">
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
            <div className="relative">
              <label htmlFor="account-search" className="sr-only">
                Search account
              </label>
              <Search className="pointer-events-none absolute left-5 top-1/2 h-5 w-5 -translate-y-1/2 text-black" />
              <input
                id="account-search"
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
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

          {searchError && (
            <div className="mt-6 rounded-lg border-2 border-black bg-red-200 p-4 shadow-neo-md">
              <p className="font-bold text-black">{searchError}</p>
            </div>
          )}

          {searchResults.length > 0 && (
            <div className="glass mt-6 overflow-hidden rounded-lg">
              <div className="border-b-2 border-black bg-yellow-200 p-5">
                <h2 className="hero-font text-3xl font-black text-black">
                  Search Results ({searchResults.length})
                </h2>
              </div>
              <div className="divide-y-2 divide-black">
                {searchResults.map((account) => (
                  <div
                    key={account.x_user_id}
                    className="flex flex-col gap-5 p-5 transition-colors hover:bg-cyan-200 sm:flex-row sm:items-center sm:justify-between"
                  >
                    <div className="flex min-w-0 flex-1 items-center gap-3">
                      <img
                        src={account.profile_image_url || getXAvatarUrl(account.x_handle)}
                        alt={`@${account.x_handle}`}
                        className="h-11 w-11 shrink-0 rounded-md border-2 border-black bg-white object-cover shadow-neo-sm"
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
      </div>
    </main>
  );
};
