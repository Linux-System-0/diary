import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

export default defineConfig({
  resolve: {
    alias: {
      '#lib': fileURLToPath(new URL('./src/lib', import.meta.url))
    }
  },
  plugins: [
    tailwindcss(),
    sveltekit({
      adapter: adapter({ fallback: 'index.html', strict: false })
    })
  ],
  server: {
    proxy: {
      '/api': {
        target: process.env.DIARY_API_TARGET ?? 'http://127.0.0.1:8787',
        changeOrigin: false
      }
    }
  },
  preview: {
    proxy: {
      '/api': {
        target: process.env.DIARY_API_TARGET ?? 'http://127.0.0.1:8787',
        changeOrigin: false
      }
    }
  }
});
