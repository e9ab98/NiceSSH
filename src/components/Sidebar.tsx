import { NavLink } from 'react-router-dom';
import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { FolderGit2, Key, FileCode2, History, Settings as SettingsIcon, UserCircle2, type LucideIcon } from 'lucide-react';
import { cn } from '../lib/utils';

interface Item {
  to: string;
  labelKey: string;
  icon: LucideIcon;
}

const items: Item[] = [
  { to: '/projects', labelKey: 'sidebar.projects', icon: FolderGit2 },
  { to: '/identities', labelKey: 'sidebar.identities', icon: UserCircle2 },
  { to: '/keys', labelKey: 'sidebar.sshKeys', icon: Key },
  { to: '/config', labelKey: 'sidebar.sshConfig', icon: FileCode2 },
  { to: '/history', labelKey: 'sidebar.history', icon: History },
  { to: '/settings', labelKey: 'sidebar.settings', icon: SettingsIcon },
];

export function Sidebar() {
  const { t } = useTranslation();
  return (
    <nav role="nav" className="w-56 border-r border-border bg-bg-1 flex flex-col p-3 gap-1 shrink-0">
      <div className="px-2 py-3 text-text-0 font-semibold tracking-tight">{t('app.name')}</div>
      {items.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          className={({ isActive }) =>
            cn(
              'relative flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors',
              isActive ? 'text-text-0' : 'text-text-1 hover:text-text-0 hover:bg-bg-2'
            )
          }
        >
          {({ isActive }) => (
            <>
              {isActive && (
                <motion.div
                  layoutId="sidebar-indicator"
                  className="absolute left-0 top-1 bottom-1 w-0.5 bg-accent rounded"
                  transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                />
              )}
              <item.icon className="h-4 w-4" />
              <span>{t(item.labelKey)}</span>
            </>
          )}
        </NavLink>
      ))}
    </nav>
  );
}
