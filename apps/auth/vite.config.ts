import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

export default defineConfig({
  base: '/proteus/auth/',
  plugins: [solid()],
  clearScreen: false,
  server: { host: '127.0.0.1', port: 1421, strictPort: true },
  preview: { host: '127.0.0.1', port: 1421, strictPort: true },
  build: { target: 'es2022', modulePreload: false, assetsInlineLimit: 0 },
});
