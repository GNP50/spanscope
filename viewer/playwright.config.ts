import { defineConfig } from '@playwright/test';
import { existsSync } from 'node:fs';

const localChrome = process.env.SPANSCOPE_CHROME ?? (existsSync('/usr/bin/google-chrome') ? '/usr/bin/google-chrome' : undefined);

export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.spec.ts',
  use: { browserName: 'chromium', headless: true, launchOptions: localChrome ? { executablePath: localChrome } : {} },
  timeout: 30_000,
});
