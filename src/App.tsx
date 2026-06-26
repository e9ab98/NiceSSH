import { useCallback } from 'react';
import { Toaster } from 'sonner';
import { ThemeProvider } from './components/ThemeProvider';
import { Sidebar } from './components/Sidebar';
import { Route, Routes, Navigate, useNavigate } from 'react-router-dom';
import { useUpdateCheck } from './hooks/useUpdateCheck';
import { useHotkeys } from './hooks/useHotkeys';
import { ProjectsView } from './views/ProjectsView';
import { IdentitiesView } from './views/IdentitiesView';
import { SshConfigView } from './views/SshConfigView';
import { HistoryView } from './views/HistoryView';
import { SettingsView } from './views/SettingsView';

export default function App() {
  useUpdateCheck();
  const navigate = useNavigate();

  // App-wide keyboard shortcuts.
  // `mod` = Cmd on macOS, Ctrl on Windows/Linux.
  useHotkeys({
    'mod+1': useCallback(() => navigate('/projects'), [navigate]),
    'mod+2': useCallback(() => navigate('/identities'), [navigate]),
    'mod+3': useCallback(() => navigate('/config'), [navigate]),
    'mod+4': useCallback(() => navigate('/history'), [navigate]),
    'mod+,': useCallback(() => navigate('/settings'), [navigate]),
  });

  return (
    <ThemeProvider>
      <Toaster richColors position="bottom-right" />
      <div className="shell">
        <div className="ambient" aria-hidden />
        <div className="relative z-10 flex h-screen text-text-0">
          <Sidebar />
          <main className="flex-1 overflow-auto">
            <Routes>
              <Route path="/" element={<Navigate to="/projects" replace />} />
              <Route path="/projects" element={<ProjectsView />} />
              <Route path="/identities" element={<IdentitiesView />} />
              <Route path="/config" element={<SshConfigView />} />
              <Route path="/history" element={<HistoryView />} />
              <Route path="/settings" element={<SettingsView />} />
            </Routes>
          </main>
        </div>
      </div>
    </ThemeProvider>
  );
}
