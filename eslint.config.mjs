import js from '@eslint/js';
import ts from 'typescript-eslint';
import solid from 'eslint-plugin-solid';
import globals from 'globals';
export default ts.config(
  {
    ignores: [
      '**/node_modules/**',
      '**/dist/**',
      '**/target/**',
      '**/gen/**',
      '.cache/**',
      '.beads/**',
      'playwright-report/**',
      'test-results/**',
    ],
  },
  { linterOptions: { reportUnusedDisableDirectives: 'error' } },
  js.configs.recommended,
  ...ts.configs.recommended,
  {
    files: ['**/*.{mjs,ts,tsx}'],
    languageOptions: { globals: { ...globals.node, ...globals.browser } },
  },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [ts.configs.recommendedTypeChecked],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    rules: {
      '@typescript-eslint/consistent-type-imports': 'error',
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/no-floating-promises': ['error', { ignoreVoid: false }],
      complexity: ['error', { max: 25, variant: 'modified' }],
      'max-depth': ['error', 4],
    },
  },
  { files: ['**/*.tsx'], ...solid.configs['flat/typescript'] },
);
