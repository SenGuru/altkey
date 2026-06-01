import { Outlet } from 'react-router-dom';
import { Nav } from './components/Nav';

export function App() {
  return (
    <div style={{ minHeight: '100vh', fontFamily: 'monospace' }}>
      <Nav />
      <main style={{ padding: '2rem' }}>
        <Outlet />
      </main>
    </div>
  );
}
