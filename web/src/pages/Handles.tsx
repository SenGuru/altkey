import { useState, useEffect, useRef } from 'react';
import { useHandles, useCreateHandle, useDeleteHandle } from '../lib/queries';
import { handleAvailability } from '../client/services.gen';
import type { HandleView } from '../client/types.gen';

// Validate handle names: lowercase letters, digits, hyphens;
// 1–63 chars; no leading or trailing hyphen.
function validateHandle(name: string): string | null {
  if (!name) return null; // empty — show nothing
  if (name.length > 63) return 'Too long (max 63 characters)';
  if (!/^[a-z0-9-]+$/.test(name)) return 'Only lowercase letters, digits, and hyphens allowed';
  if (name.startsWith('-') || name.endsWith('-')) return 'Cannot start or end with a hyphen';
  return null; // valid format
}

type Availability = 'idle' | 'checking' | 'available' | 'taken' | 'error';

export function Handles() {
  const { data: handles = [], isPending } = useHandles();
  const createHandle = useCreateHandle();
  const deleteHandle = useDeleteHandle();

  const [name, setName] = useState('');
  const [availability, setAvailability] = useState<Availability>('idle');
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Debounced availability check whenever `name` changes.
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);

    const validationError = validateHandle(name);
    if (!name || validationError) {
      setAvailability('idle');
      return;
    }

    setAvailability('checking');
    debounceRef.current = setTimeout(() => {
      handleAvailability({ query: { name } })
        .then(({ data, error }) => {
          if (error || !data) {
            setAvailability('error');
          } else {
            setAvailability(data.available ? 'available' : 'taken');
          }
        })
        .catch(() => setAvailability('error'));
    }, 400);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [name]);

  const validationError = validateHandle(name);
  const canClaim =
    name.length > 0 &&
    !validationError &&
    availability === 'available' &&
    !createHandle.isPending;

  function handleClaim(e: React.FormEvent) {
    e.preventDefault();
    if (!canClaim) return;
    createHandle.mutate(
      { name },
      {
        onSuccess: () => {
          setName('');
          setAvailability('idle');
        },
      },
    );
  }

  function handleRevoke(h: HandleView) {
    if (!window.confirm(`Revoke handle "${h.name}"? This cannot be undone.`)) return;
    deleteHandle.mutate(h.id);
  }

  function availabilityHint(): React.ReactNode {
    if (validationError) return <span className="hint hint-error">{validationError}</span>;
    if (availability === 'checking') return <span className="hint hint-info">Checking…</span>;
    if (availability === 'available') return <span className="hint hint-success">✓ Available</span>;
    if (availability === 'taken') return <span className="hint hint-error">✗ Already taken</span>;
    if (availability === 'error') return <span className="hint hint-error">Could not check availability</span>;
    return null;
  }

  return (
    <div className="page-root">
      <h2 className="page-title">Handles</h2>
      <p className="page-subtitle">
        A handle gives you a subdomain on the altkey tunnel network (
        <code>https://&lt;name&gt;.altkey.app/v1</code>).
      </p>

      {/* Claim form */}
      <section className="card">
        <h3 className="card-heading">Claim a handle</h3>
        <form className="form-row" onSubmit={handleClaim} noValidate>
          <div className="input-group">
            <input
              className="input"
              type="text"
              placeholder="my-handle"
              value={name}
              onChange={(e) => setName(e.target.value.toLowerCase())}
              aria-label="Handle name"
              maxLength={63}
              autoComplete="off"
              spellCheck={false}
            />
            {availabilityHint()}
          </div>
          <button
            className="btn btn-primary"
            type="submit"
            disabled={!canClaim}
          >
            {createHandle.isPending ? 'Claiming…' : 'Claim'}
          </button>
        </form>
        {createHandle.isError && (
          <p className="form-error" role="alert">
            {createHandle.error instanceof Error
              ? createHandle.error.message
              : 'Failed to claim handle — please try again.'}
          </p>
        )}
      </section>

      {/* Handle list */}
      <section className="card">
        <h3 className="card-heading">Your handles</h3>
        {isPending ? (
          <p className="list-loading">Loading…</p>
        ) : handles.length === 0 ? (
          <p className="list-empty">No handles yet. Claim one above.</p>
        ) : (
          <ul className="item-list">
            {handles.map((h) => (
              <li key={h.id} className="item-row">
                <div className="item-info">
                  <span className="item-name">{h.name}</span>
                  <span className={`badge badge-${h.status === 'active' ? 'active' : 'inactive'}`}>
                    {h.status}
                  </span>
                  <span className="item-meta">
                    <code>https://{h.name}.altkey.app/v1</code>
                  </span>
                </div>
                <div className="item-actions">
                  <button
                    className="btn btn-danger"
                    type="button"
                    onClick={() => handleRevoke(h)}
                    disabled={deleteHandle.isPending}
                  >
                    Revoke
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
