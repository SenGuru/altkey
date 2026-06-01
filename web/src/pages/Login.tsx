import { useState, type FormEvent } from 'react';
import { Navigate } from 'react-router-dom';
import { useMe, useRequestMagicLink } from '../lib/queries';

// OAuth providers — these are server-side redirects; full-page navigation, NOT SDK calls.
const OAUTH_PROVIDERS = [
  { label: 'Continue with Google', provider: 'google' },
  { label: 'Continue with Microsoft', provider: 'microsoft' },
  { label: 'Continue with Apple', provider: 'apple' },
  { label: 'Continue with GitHub', provider: 'github' },
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
        <header className="login-header">
          <h1 className="login-title">altkey</h1>
          <p className="login-subtitle">Sign in to your dashboard</p>
        </header>

        {/* Magic-link form */}
        <section className="login-section">
          {sent ? (
            <p className="login-sent-msg" role="status">
              Check your email for a sign-in link.
            </p>
          ) : (
            <form className="login-form" onSubmit={handleMagicLink} noValidate>
              <label htmlFor="email" className="login-label">
                Email address
              </label>
              <input
                id="email"
                className="input"
                type="email"
                placeholder="you@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                disabled={magicLinkMutation.isPending}
                required
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
        </section>

        <div className="login-divider">
          <span className="login-divider-text">or</span>
        </div>

        {/* OAuth buttons — server redirects, full-page navigation */}
        <section className="login-oauth">
          {OAUTH_PROVIDERS.map(({ label, provider }) => (
            <button
              key={provider}
              className="btn btn-oauth"
              type="button"
              onClick={() => handleOAuth(provider)}
            >
              {label}
            </button>
          ))}
        </section>
      </div>
    </div>
  );
}
