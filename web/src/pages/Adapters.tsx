import { useAdapters } from '../lib/queries';
import type { AdapterView } from '../client/types.gen';

interface AdapterCardProps {
  adapter: AdapterView;
}

function AdapterCard({ adapter }: AdapterCardProps) {
  return (
    <div className="card adapter-card">
      <div className="adapter-header">
        <h3 className="adapter-name">{adapter.name}</h3>
        <span className="badge badge-version">v{adapter.version}</span>
      </div>
      <p className="adapter-description">{adapter.description}</p>
      <div className="adapter-meta">
        <span className="adapter-meta-item">
          <span className="adapter-meta-label">Slug</span>
          <code className="mono">{adapter.slug}</code>
        </span>
        {adapter.target_tool && (
          <span className="adapter-meta-item">
            <span className="adapter-meta-label">Target tool</span>
            <code className="mono">{adapter.target_tool}</code>
          </span>
        )}
      </div>
    </div>
  );
}

export function Adapters() {
  const { data: adapters = [], isPending, isError } = useAdapters();

  return (
    <div className="page-root">
      <h2 className="page-title">Adapters</h2>
      <p className="page-subtitle">
        Adapters translate between altkey's relay protocol and each AI tool's native API format.
        They are automatically selected based on your machine's target tool.
      </p>

      {isPending ? (
        <p className="list-loading">Loading adapters…</p>
      ) : isError ? (
        <p className="list-error" role="alert">Failed to load adapters.</p>
      ) : adapters.length === 0 ? (
        <div className="card">
          <p className="list-empty">No adapters available yet.</p>
        </div>
      ) : (
        <div className="adapter-grid">
          {adapters.map((a) => (
            <AdapterCard key={a.id} adapter={a} />
          ))}
        </div>
      )}
    </div>
  );
}
