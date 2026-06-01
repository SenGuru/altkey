import { useState } from 'react';
import StatusPage from './pages/Status';
import ProvidersPage from './pages/Providers';
import KeysPage from './pages/Keys';

type Tab = 'status' | 'providers' | 'keys';

const TABS: { id: Tab; label: string }[] = [
  { id: 'status', label: 'Status' },
  { id: 'providers', label: 'Providers' },
  { id: 'keys', label: 'Keys' },
];

function App() {
  const [active, setActive] = useState<Tab>('status');

  return (
    <div className="desktop-shell">
      <aside className="desktop-sidebar">
        <div className="desktop-brand">altkey</div>
        {TABS.map((t) => (
          <button
            key={t.id}
            className={`desktop-nav-btn${active === t.id ? ' active' : ''}`}
            onClick={() => setActive(t.id)}
          >
            {t.label}
          </button>
        ))}
      </aside>

      <main className="desktop-content">
        {active === 'status' && <StatusPage />}
        {active === 'providers' && <ProvidersPage />}
        {active === 'keys' && <KeysPage />}
      </main>
    </div>
  );
}

export default App;
