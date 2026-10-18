import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { fileURLToPath } from 'node:url';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '');
  const connectSrc: string[] = ['self'];
  if (env.VITE_API_BASE?.startsWith('http')) {
    connectSrc.push(new URL(env.VITE_API_BASE).origin);
  }

  return {
    resolve: {
      alias: {
        '#lib': fileURLToPath(new URL('./src/lib', import.meta.url))
      }
    },
    plugins: [
      tailwindcss(),
      sveltekit({
        adapter: adapter({ fallback: 'index.html', strict: false }),
        csp: {
          mode: 'hash',
          directives: {
            'default-src': ['self'],
            'script-src': ['self'],
            'style-src': ['self', 'unsafe-inline'],
            'connect-src': connectSrc as never,
            'img-src': ['self', 'data:', 'https:'],
            'object-src': ['none'],
            'base-uri': ['none'],
            'frame-ancestors': ['none'],
            'form-action': ['self']
          }
        }
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
  };
});
