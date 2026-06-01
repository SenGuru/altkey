import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      // dev: proxy API + auth + webhooks to the local control plane
      '/me': 'http://127.0.0.1:8080',
      '/auth': 'http://127.0.0.1:8080',
      '/billing': 'http://127.0.0.1:8080',
      '/handles': 'http://127.0.0.1:8080',
      '/agents': 'http://127.0.0.1:8080',
      '/keys': 'http://127.0.0.1:8080',
      '/usage': 'http://127.0.0.1:8080',
      '/adapters': 'http://127.0.0.1:8080',
    },
  },
});
