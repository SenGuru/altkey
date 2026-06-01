// Configure the generated @hey-api/client-fetch client once at module load.
// baseUrl '' means same-origin — works with Vite dev-proxy (/me, /auth, etc → 127.0.0.1:8080)
// and in production (same-origin served by the Rust binary).
// credentials: 'include' ensures the session cookie is sent on every request.
import { client } from '../client/services.gen';

client.setConfig({
  baseUrl: '',
  credentials: 'include',
});

export { client };
