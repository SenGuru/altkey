import { useState } from 'react';
import { useAgents, useCreateAgent, useDeleteAgent, useHandles } from '../lib/queries';
import { SecretModal } from '../components/SecretModal';
import type { AgentView } from '../lib/queries';

export function Machines() {
  const { data: agents = [], isPending: agentsLoading } = useAgents();
  const { data: handles = [], isPending: handlesLoading } = useHandles();
  const createAgent = useCreateAgent();
  const deleteAgent = useDeleteAgent();

  const [selectedHandleId, setSelectedHandleId] = useState('');
  const [machineName, setMachineName] = useState('');
  const [pendingSecret, setPendingSecret] = useState<{ token: string } | null>(null);

  const canPair =
    selectedHandleId.length > 0 &&
    machineName.trim().length > 0 &&
    !createAgent.isPending;

  function handlePair(e: React.FormEvent) {
    e.preventDefault();
    if (!canPair) return;
    createAgent.mutate(
      { handle_id: selectedHandleId, name: machineName.trim() },
      {
        onSuccess: (created) => {
          setPendingSecret({ token: created.token });
          setMachineName('');
          setSelectedHandleId('');
        },
      },
    );
  }

  function handleUnpair(agent: AgentView) {
    if (!window.confirm(`Unpair machine "${agent.name}"? This cannot be undone.`)) return;
    deleteAgent.mutate(agent.id);
  }

  // Find the handle name for display given a handle_id
  function handleName(handleId: string): string {
    const h = handles.find((hv) => hv.id === handleId);
    return h ? h.name : handleId;
  }

  return (
    <div className="page-root">
      {pendingSecret && (
        <SecretModal
          title="Machine paired — save your agent token"
          secret={pendingSecret.token}
          instructions="Set this as ALTKEY_AGENT_TOKEN on the machine running the altkey agent. It will not be shown again."
          onClose={() => setPendingSecret(null)}
        />
      )}

      <h2 className="page-title">Machines</h2>
      <p className="page-subtitle">
        Pair a machine by giving it an agent token (<code>ak_agent_</code>).
        The agent running on that machine will authenticate with this token.
      </p>

      {/* Pair form */}
      <section className="card">
        <h3 className="card-heading">Pair a machine</h3>
        <form className="form-stack" onSubmit={handlePair} noValidate>
          <div className="field">
            <label className="label" htmlFor="handle-select">Handle</label>
            <select
              id="handle-select"
              className="input"
              value={selectedHandleId}
              onChange={(e) => setSelectedHandleId(e.target.value)}
              disabled={handlesLoading}
            >
              <option value="">— pick a handle —</option>
              {handles.map((h) => (
                <option key={h.id} value={h.id}>{h.name}</option>
              ))}
            </select>
            {!handlesLoading && handles.length === 0 && (
              <p className="field-hint">
                No handles yet — <a href="/handles">claim one first</a>.
              </p>
            )}
          </div>

          <div className="field">
            <label className="label" htmlFor="machine-name">Machine name</label>
            <input
              id="machine-name"
              className="input"
              type="text"
              placeholder="my-workstation"
              value={machineName}
              onChange={(e) => setMachineName(e.target.value)}
              autoComplete="off"
            />
          </div>

          <button
            className="btn btn-primary"
            type="submit"
            disabled={!canPair}
          >
            {createAgent.isPending ? 'Pairing…' : 'Pair machine'}
          </button>
        </form>
        {createAgent.isError && (
          <p className="form-error" role="alert">
            {createAgent.error instanceof Error
              ? createAgent.error.message
              : 'Failed to pair machine — please try again.'}
          </p>
        )}
      </section>

      {/* Agent list */}
      <section className="card">
        <h3 className="card-heading">Paired machines</h3>
        {agentsLoading ? (
          <p className="list-loading">Loading…</p>
        ) : agents.length === 0 ? (
          <p className="list-empty">No machines paired yet.</p>
        ) : (
          <ul className="item-list">
            {agents.map((agent) => (
              <li key={agent.id} className="item-row">
                <div className="item-info">
                  <span className="item-name">{agent.name}</span>
                  <span className={`badge badge-${agent.status === 'active' ? 'active' : 'inactive'}`}>
                    {agent.status}
                  </span>
                  <span className="item-meta">
                    token: <code>{agent.token_prefix}…</code>
                  </span>
                  <span className="item-meta">
                    handle: <code>{handleName(agent.handle_id)}</code>
                  </span>
                </div>
                <div className="item-actions">
                  <button
                    className="btn btn-danger"
                    type="button"
                    onClick={() => handleUnpair(agent)}
                    disabled={deleteAgent.isPending}
                  >
                    Unpair
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
