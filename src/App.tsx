import { Toaster } from 'sonner';
import { ThemeProvider } from './components/ThemeProvider';
import { Sidebar } from './components/Sidebar';
import { Route, Routes, Navigate } from 'react-router-dom';
import { useUpdateCheck } from './hooks/useUpdateCheck';
import { ProjectsView } from './views/ProjectsView';
import { IdentitiesView } from './views/IdentitiesView';
import { SshKeysView } from './views/SshKeysView';
import { SshConfigView } from './views/SshConfigView';
import { HistoryView } from './views/HistoryView';
import { SettingsView } from './views/SettingsView';

export default function App() {
  useUpdateCheck();
  return (
    <ThemeProvider>
      <Toaster richColors position="bottom-right" />
      <div className="flex h-screen bg-bg-0 text-text-0">
        <Sidebar />
        <main className="flex-1 overflow-auto">
          <Routes>
            <Route path="/" element={<Navigate to="/projects" replace />} />
            <Route path="/projects" element={<ProjectsView />} />
            <Route path="/identities" element={<IdentitiesView />} />
            <Route path="/keys" element={<SshKeysView />} />
            <Route path="/config" element={<SshConfigView />} />
            <Route path="/history" element={<HistoryView />} />
            <Route path="/settings" element={<SettingsView />} />
          </Routes>
        </main>
      </div>
    </ThemeProvider>
  );
}
