/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class', // Use class-based dark mode for better control
  theme: {
    extend: {
      fontFamily: {
        sans: ['Unbounded', 'ui-sans-serif', 'system-ui', 'sans-serif'],
        display: ['"Big Shoulders Display"', 'Unbounded', 'ui-sans-serif', 'system-ui', 'sans-serif'],
        mono: ['Unbounded'],
      },
      colors: {
        violet: {
          100: '#A5B4FB',
          200: '#A8A6FF',
          300: '#918efa',
          400: '#807dfa',
        },
        pink: {
          200: '#FFA6F6',
          300: '#fa8cef',
          400: '#fa7fee',
        },
        red: {
          200: '#FF9F9F',
          300: '#fa7a7a',
          400: '#f76363',
        },
        orange: {
          200: '#FFC29F',
          300: '#FF965B',
          400: '#fa8543',
        },
        yellow: {
          200: '#FFF59F',
          300: '#FFF066',
          400: '#FFE500',
        },
        lime: {
          100: '#c6fab4',
          200: '#B8FF9F',
          300: '#9dfc7c',
          400: '#7df752',
        },
        cyan: {
          200: '#A6FAFF',
          300: '#79F7FF',
          400: '#53f2fc',
        },
        // Grayscale brand palette
        sui: {
          50: '#fafafa',
          100: '#f5f5f5',
          200: '#e5e5e5',
          300: '#d4d4d4',
          400: '#a3a3a3',
          500: '#737373',
          600: '#525252',
          700: '#404040',
          800: '#262626',
          900: '#171717',
        },
        // Legacy accent aliases mapped to grayscale
        cyber: {
          cyan: '#e5e5e5',
          red: '#d4d4d4',
          pink: '#a3a3a3',
          green: '#f5f5f5',
        },
        // Neutral dark backgrounds
        dark: {
          900: '#050505',
          800: '#0d0d0d',
          700: '#171717',
          600: '#262626',
          500: '#404040',
        },
      },
      backgroundImage: {
        'gradient-radial': 'radial-gradient(ellipse at center, var(--tw-gradient-stops))',
        'gradient-conic': 'conic-gradient(from 180deg at 50% 50%, var(--tw-gradient-stops))',
        'sui-gradient': 'linear-gradient(135deg, #050505 0%, #262626 55%, #525252 100%)',
        'cyber-gradient': 'linear-gradient(135deg, #000000 0%, #262626 50%, #737373 100%)',
        'dark-gradient': 'linear-gradient(180deg, #0d0d0d 0%, #050505 100%)',
        'glass-gradient': 'linear-gradient(135deg, rgba(255,255,255,0.1) 0%, rgba(255,255,255,0.05) 100%)',
      },
      boxShadow: {
        'glow-sm': '0 0 15px rgba(255, 255, 255, 0.16)',
        'glow-md': '0 0 30px rgba(255, 255, 255, 0.2)',
        'glow-lg': '0 0 50px rgba(255, 255, 255, 0.24)',
        'glow-cyan': '0 0 30px rgba(229, 229, 229, 0.2)',
        'glow-pink': '0 0 30px rgba(163, 163, 163, 0.2)',
        'inner-glow': 'inset 0 0 20px rgba(255, 255, 255, 0.08)',
      },
      backdropBlur: {
        xs: '2px',
      },
      animation: {
        'pulse-slow': 'pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        'glow': 'glow 2s ease-in-out infinite alternate',
        'float': 'float 6s ease-in-out infinite',
      },
      keyframes: {
        glow: {
          '0%': { boxShadow: '0 0 20px rgba(255, 255, 255, 0.12)' },
          '100%': { boxShadow: '0 0 40px rgba(255, 255, 255, 0.24)' },
        },
        float: {
          '0%, 100%': { transform: 'translateY(0px)' },
          '50%': { transform: 'translateY(-10px)' },
        },
      },
    },
  },
  plugins: [],
}
