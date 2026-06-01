import { useEffect, useState } from 'react';
import { AgentStatus, agentStatus, startAgent, startTunnel, stopTunnel } from '../lib/bridge';

const POLL_INTERVAL_MS = 3000;

export default function StatusPage() {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  const fetchStatus = async () => {
    try {
      const s = await agentStatus();
      setStatus(s);
      setError(null);
    } catch (e: unknown) {
      setError('Agent unreachable — is the altkey agent running?');
      setStatus({ running: false, tunnel_up: false, handle: null, reachable_url: null });
    }
  };

  useEffect(() => {
    fetchStatus();
    const id = setInterval(fetchStatus, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, []);

  const handleCopy = async () => {
    if (!status?.reachable_url) return;
    try {
      await navigator.clipboard.writeText(status.reachable_url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      // clipboard may be unavailable
    }
  };

  const act = async (label: string, fn: () => Promise<void>) => {
    setBusy(label);
    try {
      await fn();
      await fetchStatus();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const running = status?.running ?? false;
  const tunnelUp = status?.tunnel_up ?? false;
  const reachableUrl = status?.reachable_url ?? null;

  return (
    <div className="page-root">
      <h1 className="page-title">Status</h1>
      <p className="page-subtitle">This machine's altkey agent and tunnel state.</p>

      {error && (
        <div className="card" style={{ borderColor: 'rgba(217,83,79,0.35)' }}>
          <span className="list-error">{error}</span>
        </div>
      )}

      <div className="card">
        <div className="card-heading">Agent</div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem', marginBottom: '1rem' }}>
          <span
            style={{
              width: 10,
              height: 10,
              borderRadius: '50%',
              background: running ? 'var(--green)' : 'var(--red)',
              flexShrink: 0,
              display: 'inline-block',
            }}
          />
          <span style={{ color: running ? 'var(--green)' : 'var(--red)', fontWeight: 600 }}>
            {running ? 'Running' : 'Not running'}
          </span>
        </div>
        {!running && (
          <button
            className="btn btn-primary btn-sm"
            disabled={busy === 'start-agent'}
            onClick={() => act('start-agent', startAgent)}
          >
            {busy === 'start-agent' ? 'Starting…' : 'Start agent'}
          </button>
        )}
      </div>

      <div className="card">
        <div className="card-heading">Tunnel</div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem', marginBottom: '1rem' }}>
          <span
            style={{
              width: 10,
              height: 10,
              borderRadius: '50%',
              background: tunnelUp ? 'var(--green)' : 'var(--text-disabled)',
              flexShrink: 0,
              display: 'inline-block',
            }}
          />
          <span style={{ color: tunnelUp ? 'var(--green)' : 'var(--text-muted)', fontWeight: 600 }}>
            {tunnelUp ? 'Up' : 'Down'}
          </span>
        </div>

        {reachableUrl && (
          <div className="tunnel-section">
            <div className="tunnel-hint">Reachable URL — use as <code className="mono">OPENAI_BASE_URL</code>:</div>
            <div className="tunnel-url-row">
              <span className="tunnel-url">{reachableUrl}</span>
              <button className="btn btn-secondary btn-sm" onClick={handleCopy}>
                {copied ? 'Copied!' : 'Copy'}
              </button>
            </div>
          </div>
        )}

        <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap', marginTop: '0.75rem' }}>
          <button
            className="btn btn-primary btn-sm"
            disabled={!running || tunnelUp || busy === 'start-tunnel'}
            onClick={() => act('start-tunnel', startTunnel)}
          >
            {busy === 'start-tunnel' ? 'Starting…' : 'Start tunnel'}
          </button>
          <button
            className="btn btn-danger btn-sm"
            disabled={!tunnelUp || busy === 'stop-tunnel'}
            onClick={() => act('stop-tunnel', stopTunnel)}
          >
            {busy === 'stop-tunnel' ? 'Stopping…' : 'Stop tunnel'}
          </button>
        </div>
      </div>
    </div>
  );
}
