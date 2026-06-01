import { useEffect, useState } from 'react';
import { agentStatus, openWeb } from '../lib/bridge';

export default function KeysPage() {
  const [baseUrl, setBaseUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    agentStatus()
      .then((s) => setBaseUrl(s.reachable_url))
      .catch(() => setBaseUrl(null));
  }, []);

  const handleMint = async () => {
    try {
      await openWeb('/keys');
    } catch {
      // Tauri not available
    }
  };

  const handleCopy = async () => {
    if (!baseUrl) return;
    try {
      await navigator.clipboard.writeText(baseUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      // clipboard unavailable
    }
  };

  return (
    <div className="page-root">
      <h1 className="page-title">Keys</h1>
      <p className="page-subtitle">
        Mint API keys in the web dashboard. Use the base URL below as your
        <code className="mono" style={{ marginLeft: 4, marginRight: 4 }}>OPENAI_BASE_URL</code>
        to route any OpenAI-compatible client through this machine's altkey agent.
      </p>

      <div className="card">
        <div className="card-heading">Mint a key</div>
        <p style={{ fontSize: 13, color: 'var(--text-secondary)', lineHeight: 1.6, marginBottom: '1rem' }}>
          Keys are minted and managed in the web dashboard. Each key is scoped to your account and
          validates against the altkey authority before forwarding to this machine's agent.
        </p>
        <button className="btn btn-primary" onClick={handleMint}>
          Mint keys in the web dashboard
        </button>
      </div>

      <div className="card">
        <div className="card-heading">Base URL for this machine</div>
        {baseUrl ? (
          <>
            <div className="tunnel-hint" style={{ marginBottom: '0.75rem' }}>
              Set <code className="mono">OPENAI_BASE_URL</code> to:
            </div>
            <div className="tunnel-url-row">
              <span className="tunnel-url">{baseUrl}</span>
              <button className="btn btn-secondary btn-sm" onClick={handleCopy}>
                {copied ? 'Copied!' : 'Copy'}
              </button>
            </div>
          </>
        ) : (
          <p className="list-empty">
            Tunnel is not active. Start the tunnel on the Status tab to get your reachable URL.
          </p>
        )}
      </div>
    </div>
  );
}
