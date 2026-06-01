import { openWeb } from '../lib/bridge';

export default function ProvidersPage() {
  const handleManage = async () => {
    try {
      await openWeb('/providers');
    } catch {
      // If Tauri invoke is unavailable (browser preview), fall through silently
    }
  };

  return (
    <div className="page-root">
      <h1 className="page-title">Providers</h1>
      <p className="page-subtitle">
        AI providers (OpenAI, Anthropic, Groq, etc.) are connected locally on this machine.
        The altkey agent on this device holds the provider credentials — they never leave your machine.
      </p>

      <div className="card">
        <div className="card-heading">How it works</div>
        <p style={{ fontSize: 13.5, color: 'var(--text-secondary)', lineHeight: 1.6, marginBottom: '1rem' }}>
          Provider API keys are stored in <code className="mono">~/.codex/auth.json</code> on this machine.
          You can add, rotate, or remove keys from the web dashboard — altkey will sync them to the local
          agent automatically when the tunnel is active.
        </p>
        <button className="btn btn-primary" onClick={handleManage}>
          Manage providers in the web app
        </button>
      </div>

      <div className="card">
        <div className="card-heading">Local connect note</div>
        <p style={{ fontSize: 13, color: 'var(--text-secondary)', lineHeight: 1.6 }}>
          Provider credentials are stored only on this machine and forwarded through the tunnel.
          No keys are stored on the altkey cloud — the tunnel proxies requests from your clients
          to this machine's agent, which holds the actual provider credentials.
        </p>
      </div>
    </div>
  );
}
