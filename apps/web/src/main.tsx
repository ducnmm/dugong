import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { SuiClientProvider, WalletProvider, lightTheme } from '@mysten/dapp-kit';
import type { ThemeVars } from '@mysten/dapp-kit';
import { getFullnodeUrl } from '@mysten/sui/client';
import { ThemeProvider } from './contexts/ThemeContext';
import '@mysten/dapp-kit/dist/index.css';
import './index.css';
import App from './App.tsx';

// Create a client for React Query.
// Screens fetch when mounted; mutations explicitly invalidate the data they change.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
    },
  },
});

// Configure Sui network
// getFullnodeUrl points at fullnode.*.sui.io, which stopped serving
// JSON-RPC in July 2026 (gRPC only now); use public nodes that still speak it.
const networks = {
  devnet: { url: getFullnodeUrl('devnet') },
  testnet: { url: 'https://sui-testnet-rpc.publicnode.com' },
  mainnet: { url: 'https://sui-rpc.publicnode.com' },
};

const walletTheme: ThemeVars = {
  ...lightTheme,
  backgroundColors: {
    ...lightTheme.backgroundColors,
    primaryButton: '#A6FAFF',
    primaryButtonHover: '#79F7FF',
    outlineButtonHover: '#FFF59F',
    modalOverlay: 'rgba(0 0 0 / 55%)',
    modalPrimary: '#ffffff',
    modalSecondary: '#B8FF9F',
    iconButtonHover: '#FFF59F',
    dropdownMenuSeparator: '#000000',
    walletItemHover: '#A6FAFF',
  },
  colors: {
    ...lightTheme.colors,
    primaryButton: '#000000',
    outlineButton: '#000000',
    iconButton: '#000000',
    body: '#000000',
    bodyMuted: '#3f3f46',
    bodyDanger: '#000000',
  },
  typography: {
    ...lightTheme.typography,
    fontFamily: 'Unbounded, ui-sans-serif, system-ui, sans-serif',
    letterSpacing: '0',
  },
};

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <SuiClientProvider networks={networks} defaultNetwork="testnet">
          <WalletProvider autoConnect theme={walletTheme}>
            <App />
          </WalletProvider>
        </SuiClientProvider>
      </QueryClientProvider>
    </ThemeProvider>
  </StrictMode>
);
