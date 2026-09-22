import { defineConfig, mergeConfig } from 'vite';
import appConfig from './vite.config';

// Defense in depth: a missing browser fixture must never reach the real backend.
export default mergeConfig(
  appConfig,
  defineConfig({
    server: { host: '127.0.0.1', port: 4173, strictPort: true, proxy: {} },
    plugins: [
      {
        name: 'fixture-api-only',
        configureServer(server) {
          server.middlewares.use('/api', (_request, response) => {
            response.statusCode = 503;
            response.end('UI tests require an explicit synthetic API fixture');
          });
        },
      },
    ],
  }),
);
