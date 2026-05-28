const DEFAULT_SERVER = "http://127.0.0.1:8787";

const PROVIDERS = {
  claude: {
    name: "Claude",
    cookieDomain: "claude.ai",
    required: ["sessionKey"],
    loginUrl: "https://claude.ai/login",
  },
  chatgpt: {
    name: "ChatGPT",
    cookieDomain: "chatgpt.com",
    required: ["__Secure-next-auth.session-token"],
    loginUrl: "https://chatgpt.com/",
  },
  gemini: {
    name: "Gemini",
    cookieDomain: "google.com",
    required: ["__Secure-1PSID"],
    loginUrl: "https://gemini.google.com/app",
  },
};

const $ = (s) => document.querySelector(s);

async function getServer() {
  const { server_url } = await chrome.storage.local.get("server_url");
  return server_url || DEFAULT_SERVER;
}

async function serverStatus() {
  const server = await getServer();
  try {
    const r = await fetch(`${server}/admin/status`);
    if (!r.ok) throw new Error(String(r.status));
    const data = await r.json();
    $("#server-state").textContent = "connected";
    $("#server-state").className = "pill ok";
    return new Set((data.sessions || []).map((s) => s.provider));
  } catch (e) {
    $("#server-state").textContent = "altkey not running";
    $("#server-state").className = "pill bad";
    return null;
  }
}

function row(id, cfg, connected, serverUp) {
  const dot = connected ? "on" : "off";
  const label = !serverUp ? "—" : connected ? "connected" : "not connected";
  const action = connected
    ? `<button class="ghost" data-disconnect="${id}">Disconnect</button>`
    : `<button data-connect="${id}" ${serverUp ? "" : "disabled"}>Connect</button>`;
  return `<div class="row">
    <div><span class="dot ${dot}"></span><span class="name">${cfg.name}</span>
      <div class="meta">${label}</div></div>
    <div>${action}</div>
  </div>`;
}

async function render() {
  const live = await serverStatus();
  const serverUp = live !== null;
  $("#providers").innerHTML = Object.entries(PROVIDERS)
    .map(([id, cfg]) => row(id, cfg, serverUp && live.has(id), serverUp))
    .join("");
}

async function readCookies(cfg) {
  const cookies = await chrome.cookies.getAll({ domain: cfg.cookieDomain });
  return cookies.map((c) => ({
    name: c.name,
    value: c.value,
    domain: c.domain,
    path: c.path,
    secure: c.secure,
  }));
}

async function connect(id) {
  const cfg = PROVIDERS[id];
  const server = await getServer();
  const cookies = await readCookies(cfg);
  const have = new Set(cookies.map((c) => c.name));
  const missing = cfg.required.filter((r) => !have.has(r));

  if (missing.length) {
    // Not logged in — open the login page and tell the user to come back.
    await chrome.tabs.create({ url: cfg.loginUrl });
    toast(`Log into ${cfg.name} in the tab that opened, then click Connect again.`, "err");
    return;
  }

  try {
    const r = await fetch(`${server}/admin/capture`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        provider: id,
        cookies,
        user_agent: navigator.userAgent,
      }),
    });
    const data = await r.json();
    if (!data.ok) throw new Error(data.error || "capture failed");
    toast(`${cfg.name} connected (${data.cookie_count} cookies).`, "ok");
    await render();
  } catch (e) {
    toast(`Failed: ${e.message}`, "err");
  }
}

async function disconnect(id) {
  const server = await getServer();
  await fetch(`${server}/admin/disconnect`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ provider: id }),
  });
  await render();
}

function toast(msg, kind) {
  let el = document.querySelector(".toast");
  if (!el) {
    el = document.createElement("div");
    el.className = "toast";
    $(".wrap").appendChild(el);
  }
  el.textContent = msg;
  el.className = `toast ${kind}`;
}

document.addEventListener("click", async (e) => {
  const t = e.target;
  if (t.dataset?.connect) {
    t.disabled = true;
    t.textContent = "Connecting…";
    await connect(t.dataset.connect);
  } else if (t.dataset?.disconnect) {
    await disconnect(t.dataset.disconnect);
  } else if (t.id === "settings-toggle") {
    const s = $("#settings");
    s.hidden = !s.hidden;
    if (!s.hidden) $("#server-url").value = await getServer();
  } else if (t.id === "save-url") {
    await chrome.storage.local.set({ server_url: $("#server-url").value.trim() || DEFAULT_SERVER });
    $("#settings").hidden = true;
    await render();
  }
});

render();
