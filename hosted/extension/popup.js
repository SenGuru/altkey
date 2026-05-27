import { isOnboarded, onboard, unlock, lock, isLocked, getMeta } from "./lib/state.js";

const SERVER_ORIGIN = "https://api.altkey.app";
const DASHBOARD_URL = "https://altkey.app/app";

const $ = (sel) => document.querySelector(sel);

function show(screen) {
  for (const s of ["onboard", "recovery", "lock", "main"]) {
    document.getElementById(`screen-${s}`).hidden = s !== screen;
  }
}

async function refreshStatus() {
  const r = await chrome.runtime.sendMessage({ type: "altkey:status" });
  if (!r?.onboarded) {
    show("onboard");
    return;
  }
  if (r.locked) {
    show("lock");
    return;
  }
  show("main");
  await renderStatus();
}

async function renderStatus() {
  // Pull provider status from server.
  const meta = await getMeta();
  if (!meta) return;
  try {
    const r = await fetch(`${meta.server_origin}/sync/status?user_id=${encodeURIComponent(meta.user_id)}`);
    const data = await r.json();
    const items = ["claude", "chatgpt", "gemini"].map((p) => {
      const row = (data.sessions ?? []).find((s) => s.provider === p);
      const cls = !row ? "off" : row.stale ? "stale" : "on";
      const label = !row ? "not connected" : row.stale ? "stale" : "synced";
      return `<div class="row"><span><span class="dot ${cls}"></span>${labelFor(p)}</span><span class="sub">${label}</span></div>`;
    });
    $("#status").innerHTML = items.join("");
  } catch (e) {
    $("#status").innerHTML = `<div class="sub">offline (${e.message})</div>`;
  }
}

function labelFor(p) {
  return { claude: "Claude", chatgpt: "ChatGPT", gemini: "Gemini" }[p] ?? p;
}

$("#onboard-go").addEventListener("click", async () => {
  $("#onboard-err").textContent = "";
  const p1 = $("#onboard-pass").value;
  const p2 = $("#onboard-pass2").value;
  if (p1 !== p2) {
    $("#onboard-err").textContent = "passphrases do not match";
    return;
  }
  if (p1.length < 12) {
    $("#onboard-err").textContent = "12+ characters required";
    return;
  }
  try {
    const meta = await onboard(p1, SERVER_ORIGIN);
    // Tell the server about us.
    const r = await fetch(`${SERVER_ORIGIN}/api/signup`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        user_id: meta.user_id,
        salt_b64: meta.salt_b64,
        verifier_b64: meta.verifier_b64,
        hmac_pub_b64: meta.hmac_pub_b64,
        wrapped_k_session_b64: meta.wrapped_k_session_b64,
        proxy_k_session_b64: meta.proxy_k_session_b64,
      }),
    });
    if (!r.ok) throw new Error(await r.text());
    const j = await r.json();
    $("#recovery-code").textContent = j.recovery_code;
    show("recovery");
  } catch (e) {
    $("#onboard-err").textContent = e.message;
  }
});

$("#recovery-done").addEventListener("click", () => {
  show("main");
  renderStatus();
});

$("#lock-unlock").addEventListener("click", async () => {
  $("#lock-err").textContent = "";
  try {
    await unlock($("#lock-pass").value);
    show("main");
    renderStatus();
    chrome.runtime.sendMessage({ type: "altkey:refresh" });
  } catch (e) {
    $("#lock-err").textContent = e.message;
  }
});

$("#lock-now").addEventListener("click", () => {
  lock();
  show("lock");
});

$("#open-dashboard").addEventListener("click", () => {
  chrome.tabs.create({ url: DASHBOARD_URL });
});

refreshStatus();
