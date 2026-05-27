// Vitest global setup: extend `expect` with jest-dom matchers and ensure the
// DOM is reset between tests.
import '@testing-library/jest-dom/vitest';
import { afterEach } from 'vitest';
import { cleanup } from '@testing-library/react';

afterEach(() => {
  cleanup();
});
