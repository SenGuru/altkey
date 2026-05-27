#!/usr/bin/env bash
# Weekly warrant canary updater. Run via cron at 09:00 UTC every Monday.
#
# Requires:
#   - CANARY_PRIVATE_KEY      ed25519 priv key (hex) — same key as wrangler secret
#   - R2_ACCESS_KEY_ID        Cloudflare R2 credentials (if using R2 for the file)
#   - R2_SECRET_ACCESS_KEY
#   - openssl, curl, jq
#
# What it does:
#   1. Fetch latest Bitcoin block hash (proof of freshness).
#   2. Build the canary text block with today's date.
#   3. Sign with ed25519 priv key.
#   4. Upload signed text + signature to R2 under canary/latest.txt
#      (the Worker /canary.txt route reads from there).

set -euo pipefail

if [[ -z "${CANARY_PRIVATE_KEY:-}" ]]; then
  echo "CANARY_PRIVATE_KEY not set" >&2
  exit 1
fi

NOW="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
BTC_HASH="$(curl -fsS https://blockstream.info/api/blocks/tip/hash)"

read -r -d '' CANARY_BODY <<EOF || true
altkey warrant canary

As of $NOW, altkey has NOT received:
  - any National Security Letters
  - any FISA-court orders
  - any gag orders of any kind
  - any law enforcement subpoenas requiring covert compliance

We have not been compelled to insert backdoors into our software,
weaken our crypto, or log user cookies or prompts.

If this statement disappears or stops being updated weekly, do not
trust the service. Switch to the OSS self-host build at
https://github.com/SenGuru/altkey.

Latest BTC block hash (proof of freshness): $BTC_HASH
EOF

# Write the body to a tmpfile, then sign it.
TMP_BODY="$(mktemp)"
TMP_SIG="$(mktemp)"
trap 'rm -f "$TMP_BODY" "$TMP_SIG"' EXIT
printf '%s\n' "$CANARY_BODY" > "$TMP_BODY"

# Convert hex priv key to raw bytes and sign with ed25519.
KEY_PEM="$(mktemp)"
printf '%s' "$CANARY_PRIVATE_KEY" | xxd -r -p > "$KEY_PEM"
openssl pkeyutl -sign -rawin -inkey "$KEY_PEM" -in "$TMP_BODY" -out "$TMP_SIG"
rm -f "$KEY_PEM"

# Append the signature as base64 footer.
SIG_B64="$(base64 -w0 "$TMP_SIG")"
{
  cat "$TMP_BODY"
  echo
  echo "-----BEGIN SIGNATURE-----"
  echo "$SIG_B64"
  echo "-----END SIGNATURE-----"
} > "${TMP_BODY}.signed"

# Upload to R2 via wrangler.
wrangler r2 object put "altkey-canary/latest.txt" --file "${TMP_BODY}.signed"

echo "canary updated at $NOW"
