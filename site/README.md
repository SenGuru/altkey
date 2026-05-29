# altkey — landing page

Static HTML/CSS for [altkey.dev](https://altkey.dev).

## Local preview

```bash
cd site
python -m http.server 8989
# → http://127.0.0.1:8989
```

## What's in here

- `index.html` — the whole page in one file (semantic sections, dark-mode aware, mobile-responsive)
- `pretext.js` — text-layout helper for resize-aware hero heights
- `assets/glass-key.png` — the product mark (1.4 MB PNG, alpha channel)
- `assets/flowstate-receipt.png` — gstack design-shotgun proof

## Stack

Vanilla HTML + CSS. Zero JavaScript dependencies. Zero build step.
The page was generated end-to-end through altkey itself:
gstack `/design-shotgun` → `/v1/responses` (Codex OAuth proxy) → ChatGPT Plus sub.

## Deploying

Anywhere that serves static files: Vercel, Cloudflare Pages, GitHub Pages,
S3+CloudFront, your own box.

Default deployment: Cloudflare Pages, branch = `dev`.
