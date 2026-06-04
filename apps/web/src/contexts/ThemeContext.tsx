import React, { useEffect } from 'react';
import { ThemeContext } from './theme-context';

const applyLightTheme = () => {
  document.documentElement.classList.remove('dark');
  document.documentElement.classList.add('light');
};

export const ThemeProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  useEffect(() => {
    applyLightTheme();
  }, []);

  return (
    <ThemeContext.Provider value={{ theme: 'light', isDark: false }}>
      {children}
    </ThemeContext.Provider>
  );
};
