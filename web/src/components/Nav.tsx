import { NavLink, useNavigate } from 'react-router-dom';
import { useLogout } from '../lib/queries';

const NAV_LINKS: { to: string; label: string }[] = [
  { to: '/', label: 'Dashboard' },
  { to: '/handles', label: 'Handles' },
  { to: '/machines', label: 'Machines' },
  { to: '/keys', label: 'Keys' },
  { to: '/usage', label: 'Usage' },
  { to: '/adapters', label: 'Adapters' },
  { to: '/billing', label: 'Billing' },
];

export function Nav() {
  const navigate = useNavigate();
  const logoutMutation = useLogout();

  function handleLogout() {
    logoutMutation.mutate(undefined, {
      onSuccess: () => {
        void navigate('/login', { replace: true });
      },
    });
  }

  return (
    <nav style={{ display: 'flex', gap: '1rem', padding: '1rem', borderBottom: '1px solid #444', alignItems: 'center', flexWrap: 'wrap' }}>
      {NAV_LINKS.map(({ to, label }) => (
        <NavLink
          key={to}
          to={to}
          end={to === '/'}
          style={({ isActive }) => ({
            fontFamily: 'monospace',
            textDecoration: 'none',
            color: isActive ? '#c9a84c' : '#e0d5c5',
            fontWeight: isActive ? 700 : 400,
          })}
        >
          {label}
        </NavLink>
      ))}
      <button
        onClick={handleLogout}
        disabled={logoutMutation.isPending}
        style={{ marginLeft: 'auto', fontFamily: 'monospace', cursor: 'pointer', background: 'none', border: '1px solid #c9a84c', color: '#c9a84c', padding: '0.25rem 0.75rem', borderRadius: '4px' }}
      >
        {logoutMutation.isPending ? 'Signing out…' : 'Sign out'}
      </button>
    </nav>
  );
}
