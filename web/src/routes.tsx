import { createBrowserRouter, Navigate, Outlet } from 'react-router-dom';
import { useMe } from './lib/queries';
import { App } from './app';

// ─── Placeholder page components (real pages implemented in Tasks 3-6) ────────
function Dashboard() { return <div>Dashboard</div>; }
function Handles() { return <div>Handles</div>; }
function Machines() { return <div>Machines</div>; }
function Keys() { return <div>Keys</div>; }
function Usage() { return <div>Usage</div>; }
function Adapters() { return <div>Adapters</div>; }
function Billing() { return <div>Billing</div>; }
function Login() {
  return (
    <div style={{ padding: '2rem', fontFamily: 'monospace' }}>
      <h1>altkey — sign in</h1>
      <p>Login page placeholder.</p>
    </div>
  );
}

// ─── Auth guard ───────────────────────────────────────────────────────────────
function AuthGuard() {
  const { data: user, isPending } = useMe();

  // While the /me fetch is in-flight, render nothing (avoids flash of redirect).
  if (isPending) return null;

  // useMe() returns null on 401/network error — redirect to login.
  if (user === null || user === undefined) {
    return <Navigate to="/login" replace />;
  }

  return <Outlet />;
}

// ─── Router ───────────────────────────────────────────────────────────────────
export const router = createBrowserRouter([
  // Public routes
  { path: '/login', element: <Login /> },

  // Auth-guarded routes — all rendered inside the App shell (Nav + Outlet)
  {
    element: <AuthGuard />,
    children: [
      {
        element: <App />,
        children: [
          { path: '/', element: <Dashboard /> },
          { path: '/handles', element: <Handles /> },
          { path: '/machines', element: <Machines /> },
          { path: '/keys', element: <Keys /> },
          { path: '/usage', element: <Usage /> },
          { path: '/adapters', element: <Adapters /> },
          { path: '/billing', element: <Billing /> },
        ],
      },
    ],
  },

  // Catch-all: redirect to dashboard (guard will handle unauthenticated state)
  { path: '*', element: <Navigate to="/" replace /> },
]);
