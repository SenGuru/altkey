import { Outlet } from 'react-router-dom';
import { Nav } from './components/Nav';

export function App() {
  return (
    <div className="app-shell">
      <Nav />
      <main className="app-main">
        <Outlet />
      </main>
    </div>
  );
}
