import { useState, type FormEvent } from 'react';
import { Navigate } from 'react-router-dom';
import { useMe, useRequestMagicLink } from '../lib/queries';

// OAuth providers — these are server-side redirects; full-page navigation, NOT SDK calls.
const OAUTH_PROVIDERS = [
  { label: 'Google', provider: 'google' },
  { label: 'Microsoft', provider: 'microsoft' },
  { label: 'Apple', provider: 'apple' },
  { label: 'GitHub', provider: 'github' },
] as const;

export function Login() {
  const { data: user, isPending: meLoading } = useMe();
  const magicLinkMutation = useRequestMagicLink();

  const [email, setEmail] = useState('');
  const [sent, setSent] = useState(false);

  // Already authenticated — send to dashboard.
  if (!meLoading && user) {
    return <Navigate to="/" replace />;
  }

  function handleMagicLink(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    if (!email.trim()) return;
    magicLinkMutation.mutate(email.trim(), {
      onSuccess: () => setSent(true),
    });
  }

  function handleOAuth(provider: string) {
    window.location.href = `/auth/${provider}/start`;
  }

  return (
    <div className="login-root">
      <div className="login-card">
        <div className="login-brand">altkey</div>

        <div>
          <h1 className="login-heading">Sign in</h1>
          <p className="login-sub">Sign in to your control plane dashboard.</p>
        </div>

        {/* Magic-link form */}
        {sent ? (
          <p className="login-success" role="status">
            Check your email — a sign-in link is on its way.
          </p>
        ) : (
          <form
            style={{ display: 'flex', flexDirection: 'column', gap: '0.75rem' }}
            onSubmit={handleMagicLink}
            noValidate
          >
            <label htmlFor="login-email" style={{ fontSize: '12px', color: 'var(--text-muted)', fontWeight: 500 }}>
              Email address
            </label>
            <input
              id="login-email"
              className="input"
              type="email"
              placeholder="you@example.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              disabled={magicLinkMutation.isPending}
              required
              // eslint-disable-next-line jsx-a11y/no-autofocus
              autoFocus
            />
            {magicLinkMutation.isError && (
              <p className="login-error" role="alert">
                {magicLinkMutation.error instanceof Error
                  ? magicLinkMutation.error.message
                  : 'Something went wrong — please try again.'}
              </p>
            )}
            <button
              className="btn btn-primary"
              type="submit"
              disabled={magicLinkMutation.isPending || !email.trim()}
            >
              {magicLinkMutation.isPending ? 'Sending…' : 'Email me a sign-in link'}
            </button>
          </form>
        )}

        <div className="login-divider">or</div>

        {/* OAuth buttons — server redirects, full-page navigation */}
        <div className="oauth-grid">
          {OAUTH_PROVIDERS.map(({ label, provider }) => (
            <button
              key={provider}
              className="btn oauth-btn"
              type="button"
              onClick={() => handleOAuth(provider)}
            >
              Continue with {label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
