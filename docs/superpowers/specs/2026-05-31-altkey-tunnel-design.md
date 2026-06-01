# altkey Tunnel — Design Spec

**Date:** 2026-05-31
**Status:** Approved design, ready for implementation planning
**Supersedes:** `2026-05-31-altkey-tunnel-architecture.md` (interim draft)

**Goal:** Turn altkey from a local-only proxy into a sellable product: the AI
subscriptions a user already pays for (Claude Max, ChatGPT Plus), exposed as a
single OpenAI-compatible API key that is **reachable from anywhere** — while the
provider tokens and the inference itself **never leave the user's own machine.**

**One-line model:** *altkey is ngrok for your AI subscriptions.*

---

## The survival principle (non-negotiable, drives every decision)

A subscription is licensed to **one human, using it at human cadence.** Centralizing
(pooling tokens, proxying inference from altkey's servers) breaks that and is fatal:
IP rate-limits, account bans, ToS violation — and one detection kills every customer
at once. Therefore:

> **The provider OAuth token and the provider API call both stay on the user's own
> machine, always. altkey-cloud is never the counterparty to OpenAI/Anthropic and
> never holds a provider token.** It sells *reachability + account + control*, not
> access to AI.

Consequence: each user's traffic egresses from their own IP/identity at their own
volume. Any ban risk is **per-user and survivable**, never central and fatal.

---

## Components (4)

### 1. Calling app — the client (unmodified, third-party)
Whatever the user points at altkey: Cursor, Aider, Cline, a script, gstack, the
OpenAI/Anthropic SDKs. It is configured with **a key + a base URL** and makes normal
OpenAI-compatible requests. altkey adds nothing to it and requires no changes to it.

### 2. altkey-agent — the local proxy daemon (EXISTS, extend)
The Rust engine in `engine/` (Phase 1, already built). Headless.
- Reads + refreshes the sub's OAuth from `~/.codex/auth.json`,
  `~/.claude/.credentials.json` (local only).
- Translates OpenAI ⟷ provider and makes the **real provider call**, egress from the
  user's IP. Already supports `/v1/chat/completions`, `/v1/responses`,
  `/v1/images/generations`, `/v1/messages`, streaming, tools, vision.
- Serves `127.0.0.1:8787` for same-machine/LAN direct use (unchanged).
- **NEW:** a **tunnel client** — opens a persistent **outbound** connection to
  altkey-cloud (works behind NAT, no port-forwarding), registers the user's
  `<handle>`, and terminates TLS for `<handle>.altkey.app` locally (see "Encryption"
  — the relay never decrypts).
- **NEW:** a **license/key check** — before serving a request, confirms the
  presented endpoint key is valid and the account's subscription is active, by asking
  the control plane (cached, with an offline grace window).
- **NEW (the "just works" piece):** **transparent mode** — the agent can intercept
  `api.openai.com` / `api.anthropic.com` *on the local machine* (a hosts/DNS redirect
  to the agent + a locally-trusted CA so HTTPS terminates), so any tool on that machine
  — **including ones that hardcode the OpenAI URL, like gstack's design binary** —
  works with **zero config and no patching.** The desktop app automates the one-time
  cert + redirect setup. (Phase 0 already scaffolded this: `ALTKEY_TRANSPARENT`,
  `ALTKEY_TLS_CERT`.) See "Usage modes."

New modules: `engine/src/tunnel.rs`, `engine/src/transparent.rs` (+ license check in
`auth.rs`).

### 3. altkey-desktop — the local GUI app (NEW)
A desktop application on the user's machine, sitting alongside the agent. It is the
human-friendly front door to the local daemon. **It has most of the web app's
features, deliberately a little reduced** (the things that only make sense in the
cloud — billing, buying, cross-machine — are not here; it links out to the web for
those).
- Connect/disconnect providers (click-through OAuth instead of CLI).
- This machine's status: tunnel on/off, the reachable URL, connection health.
- This machine's endpoint keys + this machine's usage.
- Apply adapters locally.
- Start/stop the agent + tunnel.
- Talks to the agent over its local HTTP (`127.0.0.1:8787`) + local IPC; talks to the
  web app for account/login state (read-only mirror).

Recommended build: **Tauri** (wraps the Rust agent cleanly, tiny footprint) — final
call deferred to the implementation plan.

### 4. altkey-cloud — the hosted brain (NEW). Two sub-services, one deployment in v1.
**Has EVERYTHING** and is the **single source of truth.**
- **Control plane (web app + API):** accounts, login (email + GitHub), sessions,
  **billing via Polar**, the **authority that validates every endpoint key + license**,
  the `<handle>`/subdomain registry, usage metering across all machines, adapter
  catalog/delivery, the full dashboard. (Teams/SSO/audit = future.)
- **Relay / edge:** routes the public `https://<handle>.altkey.app` connection by
  **SNI, without decrypting** (TLS passthrough, see "Encryption"), and forwards the
  encrypted stream down the agent's open outbound tunnel. It enforces subscription at
  the **handle/tunnel level**: when an agent opens a tunnel and claims `<handle>`, the
  relay confirms (via the control plane) that the account owns the handle and has an
  active sub before serving it, and tears the tunnel down if the sub lapses or the
  handle is revoked. It **cannot and does not read the per-request `ak_live_` key**
  (that's inside the encrypted stream — the agent validates it). **Never holds a
  provider token, never calls a provider, never reads request/response plaintext.**
  Stateless transport; scales horizontally.

---

## The three auth layers (the boundary, locked)

| Auth | Between | Lives | Notes |
|---|---|---|---|
| **1. altkey account** | user ↔ altkey | **web app** | login + Polar subscription; produces a session |
| **2. Provider OAuth** | user ↔ OpenAI/Anthropic | **local agent only** | tokens in `~/.codex`, `~/.claude`; refreshed locally; **never sent to altkey-cloud** |
| **3. Endpoint key** (`ak_live_…`) | calling app ↔ altkey | minted by **web app**; validated by the **agent** per request (it terminates TLS and can read the key). The **relay** can't read it (passthrough) and instead enforces the sub at the **handle/tunnel level** at connect time | authorizes "active altkey subscriber," not the provider |

**Cardinal rule:** the provider token never touches altkey-cloud. The web app
authenticates *the person + their subscription*; the user's machine authenticates
*itself to the provider*. They never mix on altkey infra.

---

## Data flow

### Primary path — reachable tunnel
```
calling app
  │  POST https://sen.altkey.app/v1/chat/completions
  │  Authorization: Bearer ak_live_…
  ▼
altkey-cloud · relay
  │  1. (at tunnel connect) sen's handle is authorized — active sub, owns handle
  │  2. route THIS encrypted connection by SNI down sen's open tunnel
  │     (TLS passthrough — relay never decrypts, never reads the key)
  ▼
altkey-agent  (sen's laptop / Pi / VPS)
  │  3. terminate TLS → validate the ak_live_ key (control plane, cached)
  │  4. run through provider code → call provider with sen's sub, from sen's IP
  │     → stream the response back up the tunnel
  ▼
caller receives the OpenAI-shaped response
```

### Secondary path — same machine / LAN direct
```
calling app → http://127.0.0.1:8787/v1/...  (Bearer ak_live_…)
   agent validates key + sub against control plane (cached) → calls provider → responds
```

Both paths: **the control plane (web app) is the validation authority.** The **agent**
validates the per-request key against it on both paths (it always terminates TLS, so it
can read the key); the **relay** additionally enforces the subscription at the handle
level for the tunnel path. Same authority either way.

## Usage modes

altkey is reachable two ways, serving two different needs. **Both end at the user's
local agent**, which makes the provider call from the user's IP/sub.

| Mode | How the tool reaches altkey | UX | For |
|---|---|---|---|
| **Transparent (local)** | the agent intercepts `api.openai.com` / `api.anthropic.com` on this machine (hosts/DNS redirect + trusted local CA) | **zero config** — keep your existing OpenAI setup; even hardcoded-URL tools just work, no patching | tools running on the same machine as the agent |
| **Tunnel (remote)** | the tool points at `https://you.altkey.app/v1` with an `ak_live_` key | set base URL + key once | phone, other devices, cron, deployed apps, teammates |

Transparent mode is the "it just works like a real key" experience, but is
**local-machine-scoped** — you can only redirect DNS on the machine the agent runs on.
Tunnel mode is the "reachable from anywhere" layer. Transparent mode also **removes the
need for per-tool patching** for local use: the reason gstack's design binary needed a
source patch (it hardcoded the URL) disappears, because the hardcoded URL is exactly
what gets intercepted.

---

## Endpoint key + license lifecycle

- User subscribes on the web app → control plane records an active subscription.
- User mints an **endpoint key** (`ak_live_…`) in the web app or desktop app → stored
  in the control-plane registry, scoped to the account (and optionally to a machine).
  The user puts this key (as `OPENAI_API_KEY`) + their URL into any calling app.
- **Validation:** the **agent** checks the per-request key against the control plane
  (cached, short TTL) — it can read the key because it terminates TLS. The **relay**
  enforces the subscription at the **handle level** (at tunnel connect, and tears down
  on lapse/revoke). Revoking a key or a lapsed sub stops new requests fast (agent
  rejects the key; relay drops the tunnel).
- **Offline grace:** the agent keeps serving for a bounded window if the control plane
  is briefly unreachable, then fails closed. (Prevents a control-plane blip from
  killing every user's local proxy.)
- **License gate:** no active subscription → the agent refuses to serve or tunnel.
  Even local-only use requires a paid sub (there is no free tier — see Monetization).

---

## Encryption — "we never see your stuff," enforced by design

The trust promise is that altkey-cloud cannot read the user's prompts/completions.
Because the calling app is an **unmodified** OpenAI client (it can't do custom crypto),
the design uses **end-to-end TLS with edge passthrough**:

- The **agent** holds the TLS certificate for the user's `<handle>.altkey.app` (issued
  via ACME; altkey controls `*.altkey.app` DNS and delegates per-handle issuance to
  agents).
- The **relay** routes the inbound TLS connection to the correct agent tunnel by
  **SNI**, forwarding the raw encrypted bytes down the agent's outbound connection
  **without decrypting.** The agent terminates TLS.
- Result: the relay moves ciphertext only; it never sees plaintext prompts or
  completions, and never holds a key to read them.

(This is exactly ngrok's TLS-tunnel posture. An edge-terminated fallback — relay
decrypts in memory to forward — is simpler but breaks the trust promise; it is
**rejected** for the default. If passthrough proves operationally hard for a narrow
case, any exception must be opt-in and disclosed.)

---

## Monetization

**No free tier. License-gated from first run** — the binary checks the account's
subscription on launch; no active sub = it does not serve (on a laptop, Pi, or VPS
alike — *where the agent runs never changes the meter*). **Two plans; team is future.**

The differentiator is **throughput + reliability**, NOT machine count or "adapters"
(both rejected as fake levers — one always-on box already gives a solo user the whole
core, and transparent mode makes tools just work without patching). The **$15** tier is
the complete product for one person, with a *fair cap on tunnel throughput/concurrency*
— sized so normal use never notices and heavy daily use does. The **$25** tier lifts
that cap and adds what a user who *depends* on altkey needs. (Capping the **tunnel** is
legitimate: it reflects altkey's own relay cost. altkey never meters *inference* — the
provider already does, via the sub.)

| | **altkey — $15/mo** | **altkey Pro — $25/mo** |
|---|---|---|
| Subs → one reachable key | ✅ | ✅ |
| Transparent local mode (every tool just works, zero config) | ✅ | ✅ |
| Tunnel URL, reachable anywhere | ✅ | ✅ |
| All providers + capabilities (chat/vision/tools/images) | ✅ | ✅ |
| Run the agent anywhere (laptop / Pi / VPS) | ✅ | ✅ |
| Custom subdomain (`you.altkey.app`) | ✅ | ✅ |
| Basic usage view | ✅ | ✅ |
| **Tunnel throughput / concurrency** | fair cap | **uncapped / priority** |
| **Reliability / failover** (endpoint stays up across 2 machines) | — | ✅ |
| **Multiple named endpoints + custom domain** (`api.you.com`) | — | ✅ |
| **Advanced usage analytics** (per-tool/model, value-saved, export) | — | ✅ |
| **Priority support** | — | ✅ |
| Team seats / SSO / audit | — | *(future)* |

**Personas:**
- **$15 — "I want my subs in my tools."** Casual-to-regular dev. Sets it up, it works.
- **$25 Pro — "altkey is critical infrastructure for me."** Heavy daily use; can't
  tolerate throttling or downtime; wants control + support. (Later: "…and my team.")

**Design principle:** the $15 throughput cap *is* the product design. Without a real,
fair limit on $15, no power user has a reason to upgrade — that's the trap that killed
the "machines" and "adapters" levers. Throughput + reliability is the honest limit.

- **Launch pricing:** list **$15/mo**; a **$10/mo founding rate** for the first ~100
  users, grandfathered for life (low-friction cold-start entry without anchoring the
  value low; new users pay $15 after).
- Billing via **Polar**.
- **Team** = the future tier above $25 (or $25 becomes the team price) — the real
  expansion lever, deferred from this scope.
- No free funnel ⇒ acquisition rides the landing page + word of mouth; a future
  *time-limited trial* (not a permanent free tier) is the only "try before buy" lever,
  out of scope for now.

---

## Feature split — web (full) vs desktop (reduced)

| Capability | Web app (cloud) | Desktop app (local) |
|---|---|---|
| Account + login | ✅ | mirror (read-only) |
| Billing / buy / upgrade | ✅ | — (deep-links to web) |
| Validate keys (authority) | ✅ | — (asks the web app) |
| Connect providers | ✅ | ✅ (click-through OAuth) |
| This machine: status / keys / usage | ✅ | ✅ |
| All machines / cross-machine view | ✅ | this machine only |
| Mint / revoke endpoint keys | ✅ | ✅ (this machine) |
| Adapters | ✅ (catalog + manage) | ✅ (apply locally) |
| Tunnel on/off + URL | ✅ | ✅ |

---

## Reused vs new

**Reused (already built, `engine/`):** provider modules (`claude_oauth`, `chatgpt`),
OpenAI⟷provider translation, OAuth read/refresh (incl. write-back to `~/.codex`),
`/v1/*` surface, local key store, admin endpoints.

**New:**
1. Agent: tunnel client + per-handle TLS termination + license/key check + transparent
   mode (local intercept of `api.openai.com`/`api.anthropic.com` via cert + redirect)
   (`engine/`).
2. Relay/edge service (SNI passthrough forwarder over outbound tunnels).
3. Control plane (accounts, Polar billing, key + handle registry, validation API,
   usage, adapter delivery, dashboard) — the web app.
4. Desktop GUI app (Tauri) — the reduced local front door.
5. ACME/cert pipeline for `*.altkey.app` per-handle certs.

---

## Error handling

- Agent offline / machine off → relay returns `503 agent_offline` (honest: "your
  machine is off"; the always-on answer is "run the agent on a box you keep on").
- Provider refresh fails → agent returns structured `provider_auth_expired`; dashboard
  + desktop prompt a reconnect.
- Subscription lapsed / key revoked → relay returns `402`/`403` before forwarding;
  agent fails closed after the offline-grace window.
- Tunnel drop → agent auto-reconnects with backoff; relay holds the handle briefly so
  in-flight callers see a short retry, not a hard 404.
- Control plane unreachable → cached validation + bounded offline grace; never a global
  outage of users' local proxies because of a control-plane blip.

---

## Security

- E2E TLS passthrough (above) — relay never reads plaintext.
- Endpoint key required by default; per-key scope + instant revoke.
- Provider token encrypted at rest on the agent; never transmitted off-machine.
- Agent secure defaults: bind `127.0.0.1` unless the tunnel is explicitly on; no
  inbound open ports (tunnel is outbound-only).
- "Personal cadence" mode (optional throttle/jitter) to lower the user's ban risk when
  running 24/7, plus an honest in-product warning about datacenter-IP ban risk.
- Closed-source trust earned the 1Password way: passthrough crypto (relay *can't*
  read), code signing, a published third-party security audit, network-transparency
  (a user can verify the agent only talks to provider endpoints + the relay).
- **Transparent mode installs a local CA** to terminate HTTPS for the intercepted
  provider hosts (the Charles/Proxyman technique). The desktop app automates it, but it
  is a real, **disclosed** trust step — never silent. Scoped to the user's own machine
  and their own traffic; the CA is generated locally and never shared.

---

## "Where the agent runs" is the user's choice (a spectrum)

```
laptop ───────── home server / Pi ───────── their VPS
(residential,        (residential,             (datacenter,
 on-when-working,     always-on,                always-on,
 lowest ban risk)     low ban risk)             higher ban risk)
```

altkey is portable; altkey-cloud is never in the token or inference path anywhere on
the line. The same product (binary + tunnel + control plane) sells regardless of where
the agent runs. The VPS end gives true 24/7 at higher per-user ban risk — the user's
informed choice.

---

## Honest limits (same as ngrok's)

1. **Needs a backing machine on.** Machine off → tunnel down → URL 503s. The tunnel
   solves *addressability*, not *availability*. 24/7 = run the agent on a box you keep
   on. (ngrok has the identical limit.)
2. **Server-like usage raises the user's ban risk.** Datacenter IP + 24/7 + robotic
   cadence is the most flaggable pattern. Per-user, survivable for the company;
   mitigated by "personal cadence" + honesty.
3. **Closed-source trust gap** — addressed by passthrough crypto + audit +
   network-transparency (above).

---

## Testing

- **Agent:** unit tests for tunnel framing + license-check logic; existing provider/
  translation tests stay green; a TLS-termination test with a self-issued handle cert.
- **Relay:** integration test (fake agent ↔ relay ↔ caller) asserting (a) correct
  SNI routing, (b) ciphertext-only at the relay (no plaintext ever in relay memory/
  logs), (c) key/sub rejection before forwarding.
- **Control plane:** tests for account/billing webhooks (Polar), key mint/validate/
  revoke, offline-grace behavior.
- **End-to-end:** real agent ↔ real relay ↔ a real tool (e.g. gstack/curl), verifying a
  `/v1/chat/completions` round-trips through `you.altkey.app` and the relay logs show
  no plaintext.

---

## Future (noted, out of current scope)

Teams / SSO / audit log; time-limited trial; Gemini (only via a clean OAuth path);
one-click "deploy the agent to a box you keep on" recipes; mobile companion.
