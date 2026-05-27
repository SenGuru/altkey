import { defineConfig } from "astro/config";

export default defineConfig({
  output: "static",
  site: "https://altkey.app",
  trailingSlash: "never",
  vite: {
    server: {
      // The dashboard talks to the Workers backend; configure this in
      // hosting (Cloudflare Pages) to use the api subdomain.
    },
  },
});
