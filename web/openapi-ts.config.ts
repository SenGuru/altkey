import { defineConfig } from '@hey-api/openapi-ts';

export default defineConfig({
  input: './openapi.json',
  output: {
    path: './src/client',
    format: 'prettier',
  },
  client: '@hey-api/client-fetch',
});
