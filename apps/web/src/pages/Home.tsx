import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle';
import { Header } from '../components/Header';
import Lottie from 'lottie-react';
import {
  Search,
  Send,
  Shield,
  ExternalLink,
  CheckCircle,
  ArrowDown,
  Trophy,
  Megaphone,
} from 'lucide-react';

// Import Lottie animations
import successAnimation from '../assets/animations/success.json';
import blockchainAnimation from '../assets/animations/blockchain.json';

interface AccountSearchResult {
  x_user_id: string;
  x_handle: string;
  sui_object_id: string;
  owner_address?: string;
}

// Tweet Card Component
const TweetCard: React.FC<{
  handle: string;
  content: React.ReactNode;
  isReply?: boolean;
  isSuccess?: boolean;
  delay?: number;
}> = ({ handle, content, isReply, isSuccess, delay = 0 }) => {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const timer = setTimeout(() => setVisible(true), delay);
    return () => clearTimeout(timer);
  }, [delay]);

  return (
    <div
      className={`glass rounded-xl p-4 transition-all duration-500 ${
        visible ? 'opacity-100 translate-y-0' : 'opacity-0 translate-y-4'
      } ${isSuccess ? 'border-cyber-green/30 bg-cyber-green/5' : ''}`}
    >
      <div className="flex items-start gap-3">
        <div className={`w-10 h-10 rounded-full flex items-center justify-center text-white font-bold ${
          isReply ? 'bg-sui-gradient' : 'bg-gray-600'
        }`}>
          {handle[0].toUpperCase()}
        </div>
        <div className="flex-1">
          <div className="flex items-center gap-2 mb-1">
            <span className="font-semibold text-white">@{handle}</span>
            {isReply && (
              <span className="text-xs text-sui-400 flex items-center gap-1">
                <CheckCircle className="w-3 h-3" /> Verified
              </span>
            )}
            <span className="text-gray-500 text-sm">· just now</span>
          </div>
          <div className="text-gray-300">{content}</div>
          {isSuccess && (
            <a
              href="#"
              className="text-sui-400 text-sm hover:text-sui-300 flex items-center gap-1 mt-2"
            >
              View on Explorer <ExternalLink className="w-3 h-3" />
            </a>
          )}
        </div>
      </div>
    </div>
  );
};

// Animated Flow Step
const FlowStep: React.FC<{
  number: number;
  title: string;
  description: string;
  delay?: number;
}> = ({ number, title, description, delay = 0 }) => {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const timer = setTimeout(() => setVisible(true), delay);
    return () => clearTimeout(timer);
  }, [delay]);

  return (
    <div
      className={`flex items-start gap-4 transition-all duration-500 ${
        visible ? 'opacity-100 translate-x-0' : 'opacity-0 -translate-x-4'
      }`}
    >
      <div className="w-8 h-8 rounded-full bg-sui-gradient flex items-center justify-center text-white font-bold text-sm flex-shrink-0">
        {number}
      </div>
      <div>
        <h4 className="text-white font-semibold">{title}</h4>
        <p className="text-gray-400 text-sm">{description}</p>
      </div>
    </div>
  );
};

export const Home: React.FC = () => {
  useDocumentTitle('Dugong - Social Wallet on Sui');
  const navigate = useNavigate();

  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<AccountSearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [searchError, setSearchError] = useState<string>('');
  const [hasSearched, setHasSearched] = useState(false);

  // Handle search
  const handleSearch = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!searchQuery.trim()) {
      return;
    }

    setIsSearching(true);
    setSearchError('');
    setSearchResults([]);
    setHasSearched(true);

    try {
      const response = await fetch(`/api/accounts/search?q=${encodeURIComponent(searchQuery)}`);

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

  return (
    <div className="min-h-screen">
      <Header />

      {/* Hero Section */}
      <section className="max-w-6xl mx-auto px-4 sm:px-6 lg:px-8 py-20">
        <div className="text-center mb-16">
          <h1 className="text-5xl md:text-6xl font-bold text-white mb-6 leading-tight">
            Your <span className="text-gradient">Social Wallet</span>
            <br />on <span className="text-cyber-cyan">Sui</span>
          </h1>
          <p className="text-xl text-gray-400 max-w-2xl mx-auto mb-8">
            Send crypto to anyone on X. No wallet address needed, just their @handle.
            Powered by Nautilus Enclave for verifiable execution.
          </p>
          <div className="flex justify-center gap-4">
            <button
              onClick={() => document.getElementById('search-section')?.scrollIntoView({ behavior: 'smooth' })}
              className="btn-sui text-lg px-8 py-3"
            >
              Get Started
            </button>
            <button
              onClick={() => document.getElementById('how-it-works')?.scrollIntoView({ behavior: 'smooth' })}
              className="btn-glass text-lg px-8 py-3"
            >
              Learn More
            </button>
          </div>
        </div>
      </section>

      {/* How It Works - Animated Demo */}
      <section id="how-it-works" className="py-20 relative overflow-hidden">
        <div className="absolute inset-0 bg-sui-gradient opacity-5" />
        <div className="max-w-6xl mx-auto px-4 sm:px-6 lg:px-8 relative">
          <div className="text-center mb-16">
            <h2 className="text-3xl md:text-4xl font-bold text-white mb-4">
              How It Works
            </h2>
            <p className="text-gray-400 max-w-xl mx-auto">
              Send tokens with a simple tweet. No wallet setup required.
            </p>
          </div>

          <div className="grid lg:grid-cols-2 gap-12 items-center">
            {/* Tweet Demo */}
            <div className="space-y-4">
              <TweetCard
                handle="bob"
                content={
                  <span>
                    <span className="text-sui-400">@Dugong</span> send 5 SUI to{' '}
                    <span className="text-sui-400">@alice</span>
                  </span>
                }
                delay={0}
              />

              <div className="flex justify-center py-2">
                <ArrowDown className="w-6 h-6 text-sui-400 animate-bounce" />
              </div>

              <div className="glass-subtle rounded-xl p-4 flex items-center gap-4">
                <div className="w-16 h-16">
                  <Lottie animationData={blockchainAnimation} loop={true} />
                </div>
                <div>
                  <p className="text-white font-semibold">Processing on Sui...</p>
                  <p className="text-gray-400 text-sm">Nautilus Enclave verifying transaction</p>
                </div>
              </div>

              <div className="flex justify-center py-2">
                <ArrowDown className="w-6 h-6 text-cyber-green animate-bounce" />
              </div>

              <TweetCard
                handle="Dugong"
                isReply
                isSuccess
                content={
                  <span>
                    <span className="text-cyber-green">Successfully sent 5 SUI</span> from{' '}
                    <span className="text-sui-400">@bob</span> to{' '}
                    <span className="text-sui-400">@alice</span>
                  </span>
                }
                delay={500}
              />
            </div>

            {/* Steps */}
            <div className="space-y-8">
              <FlowStep
                number={1}
                title="Tweet Your Command"
                description="Simply mention @Dugong with your transfer command like 'send 5 SUI to @alice'"
                delay={100}
              />
              <FlowStep
                number={2}
                title="Enclave Processing"
                description="Nautilus Enclave verifies your identity, parses the command, and validates balances"
                delay={300}
              />
              <FlowStep
                number={3}
                title="On-Chain Execution"
                description="Transaction is signed and submitted to Sui blockchain with gas sponsorship"
                delay={500}
              />
              <FlowStep
                number={4}
                title="Confirmation"
                description="Receive a reply tweet with transaction confirmation and Explorer link"
                delay={700}
              />
            </div>
          </div>
        </div>
      </section>

      {/* Sample Commands */}
      <section className="py-20">
        <div className="max-w-6xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-12">
            <h2 className="text-3xl font-bold text-white mb-4">What You Can Do</h2>
            <p className="text-gray-400">Powerful commands, simple tweets</p>
          </div>

          <div className="grid md:grid-cols-2 gap-6">
            {[
              {
                title: 'Send to Handle',
                command: '@Dugong send 5 SUI to @alice',
                description: 'Transfer tokens to any X user',
              },
              {
                title: 'Send to Address',
                command: '@Dugong send 10 SUI to 0x1234...5678',
                description: 'Direct transfer to Sui address',
              },
              {
                title: 'Link Wallet',
                command: 'Dashboard -> Link Wallet',
                description: 'Connect your Sui wallet through the dApp',
              },
              {
                title: 'Withdraw Funds',
                command: '@Dugong withdraw 3 SUI',
                description: 'Withdraw to your linked wallet',
              },
            ].map((item, index) => (
              <div key={index} className="glass glass-hover rounded-xl p-6 group">
                <h3 className="text-white font-semibold mb-2">{item.title}</h3>
                <code className="block text-sui-400 bg-dark-800 rounded-lg p-3 text-sm mb-3 font-mono">
                  {item.command}
                </code>
                <p className="text-gray-400 text-sm">{item.description}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Use Cases */}
      <section className="py-20 relative">
        <div className="absolute inset-0 bg-cyber-gradient opacity-5" />
        <div className="max-w-6xl mx-auto px-4 sm:px-6 lg:px-8 relative">
          <div className="text-center mb-12">
            <h2 className="text-3xl font-bold text-white mb-4">Use Cases</h2>
            <p className="text-gray-400">From social payments to marketing campaigns</p>
          </div>

          <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6">
            {[
              {
                icon: Send,
                title: 'Social Payments',
                description: 'Send tokens to friends, family, or anyone on X',
                color: 'text-sui-400 bg-sui-500/20',
              },
              {
                icon: Trophy,
                title: 'Social Bounties',
                description: 'Reward engagement - first to retweet gets SUI',
                color: 'text-yellow-400 bg-yellow-400/20',
              },
              {
                icon: Megaphone,
                title: 'Marketing Campaigns',
                description: 'Automated rewards for hashtag campaigns',
                color: 'text-cyber-green bg-cyber-green/20',
              },
            ].map((item, index) => (
              <div key={index} className="glass glass-hover rounded-xl p-6 text-center group">
                <div className={`w-14 h-14 rounded-xl ${item.color} flex items-center justify-center mx-auto mb-4 group-hover:shadow-glow-sm transition-shadow`}>
                  <item.icon className="w-7 h-7" />
                </div>
                <h3 className="text-white font-semibold mb-2">{item.title}</h3>
                <p className="text-gray-400 text-sm">{item.description}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Search Section */}
      <section id="search-section" className="py-20">
        <div className="max-w-4xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-12">
            <h2 className="text-3xl font-bold text-white mb-4">Search Accounts</h2>
            <p className="text-gray-400">Find Dugong accounts by handle, ID, or address</p>
          </div>

          {/* Search Form */}
          <div className="glass rounded-2xl p-8 mb-8 shadow-glow-sm relative overflow-hidden">
            <div className="absolute top-0 right-0 w-32 h-32 bg-cyber-cyan/10 rounded-full blur-2xl" />
            <form onSubmit={handleSearch} className="relative">
              <div className="relative">
                <div className="absolute inset-y-0 left-0 pl-5 flex items-center pointer-events-none">
                  <Search className="h-5 w-5 text-gray-400" />
                </div>
                <input
                  type="text"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder="Search by @handle, user ID, or 0x... address"
                  className="input-glass pl-14 pr-32 py-5 text-lg rounded-xl"
                />
                <button
                  type="submit"
                  disabled={isSearching || !searchQuery.trim()}
                  className="absolute inset-y-0 right-0 px-6 m-2 btn-sui rounded-lg disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:shadow-none disabled:hover:scale-100"
                >
                  {isSearching ? (
                    <span className="flex items-center gap-2">
                      <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                      Searching
                    </span>
                  ) : (
                    'Search'
                  )}
                </button>
              </div>
            </form>
          </div>

          {/* Search Error */}
          {searchError && (
            <div className="mb-6 p-4 glass rounded-xl border-red-500/30 bg-red-500/10">
              <p className="text-red-400">{searchError}</p>
            </div>
          )}

          {/* Search Results */}
          {searchResults.length > 0 && (
            <div className="glass rounded-2xl overflow-hidden mb-8">
              <div className="p-5 border-b border-white/10">
                <h3 className="text-lg font-semibold text-white">
                  Search Results ({searchResults.length})
                </h3>
              </div>
              <div className="divide-y divide-white/5">
                {searchResults.map((account) => (
                  <div
                    key={account.x_user_id}
                    className="p-6 hover:bg-white/5 transition-colors"
                  >
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-3 mb-3">
                          <div className="w-10 h-10 rounded-full bg-sui-gradient flex items-center justify-center text-white font-bold">
                            {account.x_handle[0]?.toUpperCase()}
                          </div>
                          <div>
                            <span className="text-xl font-bold text-white">
                              @{account.x_handle}
                            </span>
                            <p className="text-sm text-gray-500">
                              ID: {account.x_user_id}
                            </p>
                          </div>
                        </div>

                        <div className="space-y-2 ml-13">
                          <div className="flex items-center gap-2">
                            <span className="text-xs font-medium text-gray-500 uppercase tracking-wide">
                              Account
                            </span>
                            <code className="text-xs text-sui-400 bg-sui-500/10 px-2 py-1 rounded-lg font-mono">
                              {account.sui_object_id.slice(0, 20)}...{account.sui_object_id.slice(-8)}
                            </code>
                          </div>

                          {account.owner_address && (
                            <div className="flex items-center gap-2">
                              <span className="text-xs font-medium text-gray-500 uppercase tracking-wide">
                                Owner
                              </span>
                              <code className="text-xs text-gray-400 bg-white/5 px-2 py-1 rounded-lg font-mono">
                                {account.owner_address.slice(0, 20)}...{account.owner_address.slice(-8)}
                              </code>
                            </div>
                          )}
                        </div>
                      </div>

                      <button
                        onClick={() => navigate(`/account/${account.x_user_id}`)}
                        className="btn-sui text-sm flex items-center gap-2"
                      >
                        View Account
                        <ExternalLink className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Empty State */}
          {!isSearching && searchResults.length === 0 && hasSearched && !searchError && (
            <div className="text-center py-12 glass rounded-2xl">
              <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-white/5 flex items-center justify-center">
                <Search className="w-8 h-8 text-gray-500" />
              </div>
              <p className="text-gray-400">
                No accounts found for "<span className="text-white">{searchQuery}</span>"
              </p>
            </div>
          )}

          {/* Feature Cards */}
          {!hasSearched && (
            <div className="grid md:grid-cols-3 gap-6 mt-12">
              <div className="glass glass-hover rounded-2xl p-6 group">
                <div className="w-12 h-12 rounded-xl bg-sui-500/20 flex items-center justify-center mb-4 group-hover:shadow-glow-sm transition-shadow">
                  <Search className="w-6 h-6 text-sui-400" />
                </div>
                <h3 className="text-lg font-semibold text-white mb-2">
                  Find Accounts
                </h3>
                <p className="text-sm text-gray-400">
                  Search for Dugong accounts by Twitter handle or Sui address
                </p>
              </div>

              <div className="glass glass-hover rounded-2xl p-6 group">
                <div className="w-12 h-12 rounded-xl bg-cyber-cyan/20 flex items-center justify-center mb-4 group-hover:shadow-glow-cyan transition-shadow">
                  <Send className="w-6 h-6 text-cyber-cyan" />
                </div>
                <h3 className="text-lg font-semibold text-white mb-2">
                  Send Tokens
                </h3>
                <p className="text-sm text-gray-400">
                  Transfer tokens to anyone on X using their Twitter handle
                </p>
              </div>

              <div className="glass glass-hover rounded-2xl p-6 group">
                <div className="w-12 h-12 rounded-xl bg-sui-500/20 flex items-center justify-center mb-4 group-hover:shadow-glow-sm transition-shadow">
                  <Shield className="w-6 h-6 text-sui-400" />
                </div>
                <h3 className="text-lg font-semibold text-white mb-2">
                  Secure & Simple
                </h3>
                <p className="text-sm text-gray-400">
                  Your assets are secured by Sui blockchain and Nautilus Enclave
                </p>
              </div>
            </div>
          )}
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-20">
        <div className="max-w-4xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="glass rounded-2xl p-12 text-center relative overflow-hidden">
            <div className="absolute inset-0 bg-sui-gradient opacity-10" />
            <div className="absolute top-0 right-0 w-64 h-64 bg-cyber-cyan/20 rounded-full blur-3xl" />
            <div className="relative">
              <div className="w-20 h-20 mx-auto mb-6">
                <Lottie animationData={successAnimation} loop={true} />
              </div>
              <h2 className="text-3xl font-bold text-white mb-4">
                Ready to Get Started?
              </h2>
              <p className="text-gray-400 mb-8 max-w-lg mx-auto">
                Join the future of social payments. Tweet to transact, no wallet setup required.
              </p>
              <div className="flex justify-center gap-4">
                <a
                  href="https://twitter.com/Dugong"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="btn-sui text-lg px-8 py-3 flex items-center gap-2"
                >
                  <svg className="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
                  </svg>
                  Follow @Dugong
                </a>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Footer */}
      <footer className="border-t border-white/5 py-8">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex flex-col md:flex-row justify-between items-center gap-4">
            <p className="text-sm text-gray-500">
              Powered by <span className="text-sui-400">Sui</span> and{' '}
              <span className="text-cyber-cyan">Nautilus Enclave</span>
            </p>
            <div className="flex items-center gap-6">
              <a href="#" className="text-sm text-gray-500 hover:text-white transition-colors">
                Documentation
              </a>
              <a href="#" className="text-sm text-gray-500 hover:text-white transition-colors">
                GitHub
              </a>
              <a href="#" className="text-sm text-gray-500 hover:text-white transition-colors">
                Twitter
              </a>
            </div>
          </div>
        </div>
      </footer>
    </div>
  );
};
