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
    <nav className="nav">
      <span className="nav-brand">altkey</span>
      <div className="nav-links">
        {NAV_LINKS.map(({ to, label }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`}
          >
            {label}
          </NavLink>
        ))}
      </div>
      <div className="nav-logout">
        <button
          className="btn btn-secondary btn-sm"
          type="button"
          onClick={handleLogout}
          disabled={logoutMutation.isPending}
        >
          {logoutMutation.isPending ? 'Signing out…' : 'Sign out'}
        </button>
      </div>
    </nav>
  );
}
