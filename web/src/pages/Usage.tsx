import { useUsageSummary } from '../lib/queries';
import type { RollupView } from '../client/types.gen';

// ─── Inline SVG bar chart ──────────────────────────────────────────────────────
// Renders a horizontal bar per rollup row (tokens per period+model bucket).
// No chart library — pure SVG computed from the data.

interface BarChartProps {
  rollups: RollupView[];
}

function TokenBarChart({ rollups }: BarChartProps) {
  if (rollups.length === 0) return null;

  const BAR_HEIGHT = 20;
  const BAR_GAP = 8;
  const LABEL_WIDTH = 120;
  const CHART_WIDTH = 320;
  const HEIGHT = rollups.length * (BAR_HEIGHT + BAR_GAP);
  const maxTokens = Math.max(...rollups.map((r) => r.sum_tokens), 1);

  return (
    <div className="chart-wrapper">
      <h4 className="chart-title">Tokens by period / model</h4>
      <svg
        width={LABEL_WIDTH + CHART_WIDTH + 60}
        height={HEIGHT}
        aria-label="Bar chart of token usage per rollup bucket"
        role="img"
      >
        {rollups.map((r, i) => {
          const y = i * (BAR_HEIGHT + BAR_GAP);
          const barW = Math.max(2, (r.sum_tokens / maxTokens) * CHART_WIDTH);
          const label = `${r.period}${r.model ? ` / ${r.model}` : ''}`;

          return (
            <g key={`${r.period}-${r.model ?? ''}-${r.tool ?? ''}`}>
              {/* Label */}
              <text
                x={LABEL_WIDTH - 6}
                y={y + BAR_HEIGHT / 2 + 5}
                textAnchor="end"
                className="chart-label"
              >
                {label.length > 18 ? label.slice(0, 17) + '…' : label}
              </text>
              {/* Bar */}
              <rect
                x={LABEL_WIDTH}
                y={y}
                width={barW}
                height={BAR_HEIGHT}
                className="chart-bar"
                rx={3}
              />
              {/* Value */}
              <text
                x={LABEL_WIDTH + barW + 6}
                y={y + BAR_HEIGHT / 2 + 5}
                className="chart-value"
              >
                {r.sum_tokens.toLocaleString()}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}

// ─── Usage page ───────────────────────────────────────────────────────────────

function fmt(n: number) {
  return n.toLocaleString();
}

function fmtBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

export function Usage() {
  const { data: rollups = [], isPending, isError } = useUsageSummary();

  const totalRequests = rollups.reduce((s, r) => s + r.sum_requests, 0);
  const totalTokens = rollups.reduce((s, r) => s + r.sum_tokens, 0);
  const totalBytes = rollups.reduce((s, r) => s + r.sum_bytes, 0);

  return (
    <div className="page-root">
      <h2 className="page-title">Usage</h2>
      <p className="page-subtitle">Aggregated usage across all your machines and keys.</p>

      {/* ── Totals header ── */}
      <section className="usage-totals">
        <div className="stat-card">
          <span className="stat-value">{fmt(totalRequests)}</span>
          <span className="stat-label">Total requests</span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{fmt(totalTokens)}</span>
          <span className="stat-label">Total tokens</span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{fmtBytes(totalBytes)}</span>
          <span className="stat-label">Total bytes</span>
        </div>
      </section>

      {isPending ? (
        <p className="list-loading">Loading usage…</p>
      ) : isError ? (
        <p className="list-error" role="alert">Failed to load usage data.</p>
      ) : rollups.length === 0 ? (
        <div className="card">
          <p className="list-empty">No usage yet. Once your machines start proxying requests, data will appear here.</p>
        </div>
      ) : (
        <>
          {/* ── Bar chart ── */}
          <div className="card">
            <TokenBarChart rollups={rollups} />
          </div>

          {/* ── Rollup table ── */}
          <div className="card">
            <h3 className="card-heading">Rollup breakdown</h3>
            <div className="table-wrapper">
              <table className="usage-table">
                <thead>
                  <tr>
                    <th>Period</th>
                    <th>Model</th>
                    <th>Tool</th>
                    <th>Provider</th>
                    <th className="num-col">Requests</th>
                    <th className="num-col">Tokens</th>
                    <th className="num-col">Bytes</th>
                  </tr>
                </thead>
                <tbody>
                  {rollups.map((r, idx) => (
                    <tr key={idx}>
                      <td>
                        <code className="mono">{r.period}</code>
                      </td>
                      <td>{r.model ?? <span className="muted">—</span>}</td>
                      <td>{r.tool ?? <span className="muted">—</span>}</td>
                      <td>{r.provider ?? <span className="muted">—</span>}</td>
                      <td className="num-col">{fmt(r.sum_requests)}</td>
                      <td className="num-col">{fmt(r.sum_tokens)}</td>
                      <td className="num-col">{fmtBytes(r.sum_bytes)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
