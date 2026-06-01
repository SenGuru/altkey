import { useState } from 'react';
import { Link } from 'react-router-dom';
import {
  useSubscription,
  useHandles,
  useAgents,
  useKeys,
} from '../lib/queries';

interface ChecklistItemProps {
  done: boolean;
  label: string;
  linkTo: string;
  linkLabel: string;
}

function ChecklistItem({ done, label, linkTo, linkLabel }: ChecklistItemProps) {
  return (
    <li className={`checklist-item${done ? ' checklist-item--done' : ''}`}>
      <span className={`checklist-icon${done ? ' checklist-icon--done' : ''}`} aria-hidden="true">
        {done ? '✓' : '○'}
      </span>
      <span className="checklist-label">{label}</span>
      {!done && (
        <Link to={linkTo} className="checklist-link">
          {linkLabel}
        </Link>
      )}
    </li>
  );
}

export function Dashboard() {
  const { data: sub, isPending: subLoading } = useSubscription();
  const { data: handles = [], isPending: handlesLoading } = useHandles();
  const { data: agents = [], isPending: agentsLoading } = useAgents();
  const { data: keys = [], isPending: keysLoading } = useKeys();

  const [copied, setCopied] = useState(false);

  const firstHandle = handles[0];
  const tunnelUrl = firstHandle ? `https://${firstHandle.name}.altkey.app/v1` : null;

  const isLoading = subLoading || handlesLoading || agentsLoading || keysLoading;

  const hasActiveSub = sub?.active === true;
  const hasHandle = handles.length > 0;
  const hasMachine = agents.length > 0;
  const hasKey = keys.length > 0;

  function handleCopyUrl() {
    if (!tunnelUrl) return;
    navigator.clipboard.writeText(tunnelUrl).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      },
      () => {},
    );
  }

  return (
    <div className="page-root">
      <h2 className="page-title">Dashboard</h2>
      <p className="page-subtitle">Your altkey control plane — one view of your tunnel network.</p>

      {isLoading ? (
        <p className="list-loading">Loading…</p>
      ) : (
        <>
          {/* ── Status overview strip ── */}
          <section className="dashboard-overview">
            {/* Subscription status */}
            <div className="card overview-card">
              <span className="overview-label">Subscription</span>
              {sub ? (
                <div className="overview-value-row">
                  <span className="overview-value">{sub.plan ?? 'None'}</span>
                  <span className={`badge ${sub.active ? 'badge-active' : 'badge-inactive'}`}>
                    {sub.active ? 'Active' : 'Inactive'}
                  </span>
                  {sub.is_founding && (
                    <span className="badge badge-founding">Founding</span>
                  )}
                </div>
              ) : (
                <div className="overview-value-row">
                  <span className="overview-value muted">No subscription</span>
                  <Link to="/billing" className="btn btn-primary btn-sm">Subscribe</Link>
                </div>
              )}
            </div>

            {/* Machine count */}
            <div className="card overview-card">
              <span className="overview-label">Machines</span>
              <div className="overview-value-row">
                <span className="overview-value overview-count">{agents.length}</span>
                <Link to="/machines" className="overview-link">Manage</Link>
              </div>
            </div>

            {/* Key count */}
            <div className="card overview-card">
              <span className="overview-label">API Keys</span>
              <div className="overview-value-row">
                <span className="overview-value overview-count">{keys.length}</span>
                <Link to="/keys" className="overview-link">Manage</Link>
              </div>
            </div>
          </section>

          {/* ── Tunnel URL ── */}
          {tunnelUrl ? (
            <section className="card tunnel-section">
              <h3 className="card-heading">Your tunnel endpoint</h3>
              <p className="tunnel-hint">
                Point any OpenAI-compatible client at this URL (as <code>OPENAI_BASE_URL</code>):
              </p>
              <div className="tunnel-url-row">
                <code className="tunnel-url">{tunnelUrl}</code>
                <button
                  className={`btn${copied ? ' btn-secondary' : ' btn-primary'} btn-sm`}
                  type="button"
                  onClick={handleCopyUrl}
                  aria-label="Copy tunnel URL"
                >
                  {copied ? 'Copied!' : 'Copy'}
                </button>
              </div>
            </section>
          ) : (
            <section className="card tunnel-section tunnel-section--empty">
              <h3 className="card-heading">Your tunnel endpoint</h3>
              <p className="list-empty">
                Claim a handle to get your tunnel URL.{' '}
                <Link to="/handles" className="inline-link">Go to Handles →</Link>
              </p>
            </section>
          )}

          {/* ── Quick-start checklist ── */}
          <section className="card quickstart-section">
            <h3 className="card-heading">Quick start</h3>
            <p className="quickstart-hint">Complete these steps to start routing AI traffic through altkey.</p>
            <ol className="checklist">
              <ChecklistItem
                done={hasActiveSub}
                label="Subscribe to a plan"
                linkTo="/billing"
                linkLabel="Choose a plan →"
              />
              <ChecklistItem
                done={hasHandle}
                label="Claim a handle (your subdomain)"
                linkTo="/handles"
                linkLabel="Claim handle →"
              />
              <ChecklistItem
                done={hasMachine}
                label="Pair a machine (copy the agent token)"
                linkTo="/machines"
                linkLabel="Pair machine →"
              />
              <ChecklistItem
                done={hasKey}
                label="Mint an API key (use as OPENAI_API_KEY)"
                linkTo="/keys"
                linkLabel="Create key →"
              />
            </ol>
            {hasActiveSub && hasHandle && hasMachine && hasKey && (
              <p className="quickstart-done">
                All steps complete — you're live on the altkey network.
              </p>
            )}
          </section>
        </>
      )}
    </div>
  );
}
