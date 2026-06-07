import React from 'react';

// Token icons
import suiIcon from '../assets/tokens/sui.png';
import walIcon from '../assets/tokens/wal.png';
import usdcIcon from '../assets/tokens/usdc.png';
import dugIcon from '../assets/tokens/dug-simple.png';

interface TokenIconProps {
  symbol: string;
  iconUrl?: string | null;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
  framed?: boolean;
}

// Known token icons by symbol
const KNOWN_ICONS: Record<string, string> = {
  DUG: dugIcon,
  SUI: suiIcon,
  WAL: walIcon,
  USDC: usdcIcon,
};

const FALLBACK_COLORS = ['#A6FAFF', '#B8FF9F', '#FFF59F', '#FFA6F6', '#A8A6FF', '#FFC29F'];

// Generate a consistent neo accent from a string
function stringToAccent(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = str.charCodeAt(i) + ((hash << 5) - hash);
  }
  return FALLBACK_COLORS[Math.abs(hash) % FALLBACK_COLORS.length];
}

export const TokenIcon: React.FC<TokenIconProps> = ({
  symbol,
  iconUrl,
  size = 'md',
  className = '',
  framed = true,
}) => {
  const sizeClasses = {
    sm: 'w-6 h-6 text-xs',
    md: 'w-8 h-8 text-sm',
    lg: 'w-12 h-12 text-lg',
  };

  // Try to get icon: custom iconUrl -> known icon -> fallback letter
  const icon = iconUrl || KNOWN_ICONS[symbol.toUpperCase()];

  if (icon) {
    return (
      <img
        src={icon}
        alt={symbol}
        className={`${sizeClasses[size]} ${
          framed
            ? 'rounded-md border border-black bg-white object-contain p-0.5 shadow-neo-sm'
            : 'object-contain'
        } ${className}`}
      />
    );
  }

  // Fallback: first letter with grayscale background
  const bgColor = stringToAccent(symbol);

  return (
    <div
      className={`${sizeClasses[size]} flex items-center justify-center font-black text-black ${
        framed ? 'rounded-md border border-black shadow-neo-sm' : ''
      } ${className}`}
      style={{ backgroundColor: bgColor }}
    >
      {symbol.charAt(0).toUpperCase()}
    </div>
  );
};

export default TokenIcon;
