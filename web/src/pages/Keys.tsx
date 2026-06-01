import { useState } from 'react';
import { useKeys, useCreateKey, useDeleteKey, useAgents, useHandles } from '../lib/queries';
import { SecretModal } from '../components/SecretModal';
import type { KeyView } from '../lib/queries';

export function Keys() {
  const { data: keys = [], isPending: keysLoading } = useKeys();
  const { data: agents = [], isPending: agentsLoading } = useAgents();
  const { data: handles = [] } = useHandles();
  const createKey = useCreateKey();
  const deleteKey = useDeleteKey();

  const [keyName, setKeyName] = useState('');
  const [selectedAgentId, setSelectedAgentId] = useState('');
  const [pendingSecret, setPendingSecret] = useState<{
    secret: string;
    handleName: string;
  } | null>(null);

  const canCreate = keyName.trim().length > 0 && !createKey.isPending;

  function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    if (!canCreate) return;

    // Find the handle name for the selected agent (for instructions in the modal)
    let hName = handles.length > 0 ? handles[0]?.name ?? 'your-handle' : 'your-handle';
    if (selectedAgentId) {
      const agent = agents.find((a) => a.id === selectedAgentId);
      if (agent) {
        const h = handles.find((hv) => hv.id === agent.handle_id);
        if (h) hName = h.name;
      }
    }

    createKey.mutate(
      {
        name: keyName.trim(),
        agent_id: selectedAgentId || null,
      },
      {
        onSuccess: (created) => {
          setPendingSecret({ secret: created.secret, handleName: hName });
          setKeyName('');
          setSelectedAgentId('');
        },
      },
    );
  }

  function handleRevoke(key: KeyView) {
    if (!window.confirm(`Revoke key "${key.name}"? This cannot be undone.`)) return;
    deleteKey.mutate(key.id);
  }

  return (
    <div className="page-root">
      {pendingSecret && (
        <SecretModal
          title="Key created — save your secret"
          secret={pendingSecret.secret}
          instructions={`Use this as OPENAI_API_KEY with base URL https://${pendingSecret.handleName}.altkey.app/v1 — it will not be shown again.`}
          onClose={() => setPendingSecret(null)}
        />
      )}

      <h2 className="page-title">Keys</h2>
      <p className="page-subtitle">
        API keys (<code>ak_live_</code>) authenticate requests proxied through your handle.
        Use them as <code>OPENAI_API_KEY</code> with the altkey base URL.
      </p>

      {/* Create key form */}
      <section className="card">
        <h3 className="card-heading">Create a key</h3>
        <form className="form-stack" onSubmit={handleCreate} noValidate>
          <div className="field">
            <label className="label" htmlFor="key-name">Key name</label>
            <input
              id="key-name"
              className="input"
              type="text"
              placeholder="production"
              value={keyName}
              onChange={(e) => setKeyName(e.target.value)}
              autoComplete="off"
            />
          </div>

          <div className="field">
            <label className="label" htmlFor="agent-select">
              Machine (optional — leave blank to allow any machine)
            </label>
            <select
              id="agent-select"
              className="input"
              value={selectedAgentId}
              onChange={(e) => setSelectedAgentId(e.target.value)}
              disabled={agentsLoading}
            >
              <option value="">— any machine —</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>{a.name}</option>
              ))}
            </select>
          </div>

          <button
            className="btn btn-primary"
            type="submit"
            disabled={!canCreate}
          >
            {createKey.isPending ? 'Creating…' : 'Create key'}
          </button>
        </form>
        {createKey.isError && (
          <p className="form-error" role="alert">
            {createKey.error instanceof Error
              ? createKey.error.message
              : 'Failed to create key — please try again.'}
          </p>
        )}
      </section>

      {/* Key list */}
      <section className="card">
        <h3 className="card-heading">Your keys</h3>
        {keysLoading ? (
          <p className="list-loading">Loading…</p>
        ) : keys.length === 0 ? (
          <p className="list-empty">No keys yet. Create one above.</p>
        ) : (
          <ul className="item-list">
            {keys.map((key) => (
              <li key={key.id} className={`item-row${key.revoked_at ? ' item-row--revoked' : ''}`}>
                <div className="item-info">
                  <span className="item-name">{key.name}</span>
                  {key.revoked_at ? (
                    <span className="badge badge-inactive">Revoked</span>
                  ) : (
                    <span className="badge badge-active">Active</span>
                  )}
                  <span className="item-meta">
                    prefix: <code>{key.key_prefix}…</code>
                  </span>
                  <span className="item-meta">
                    created: {new Date(key.created_at).toLocaleDateString()}
                  </span>
                </div>
                <div className="item-actions">
                  {!key.revoked_at && (
                    <button
                      className="btn btn-danger"
                      type="button"
                      onClick={() => handleRevoke(key)}
                      disabled={deleteKey.isPending}
                    >
                      Revoke
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
