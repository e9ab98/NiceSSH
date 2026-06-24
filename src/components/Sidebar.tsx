import { NavLink } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { FolderGit2, FileCode2, History, Settings as SettingsIcon, UserCircle2, type LucideIcon } from 'lucide-react';
import { cn } from '../lib/utils';

interface Item {
  to: string;
  labelKey: string;
  icon: LucideIcon;
}

interface Group {
  labelKey: string;
  items: Item[];
}

const groups: Group[] = [
  {
    labelKey: 'sidebar.group.git',
    items: [
      { to: '/projects', labelKey: 'sidebar.projects', icon: FolderGit2 },
      { to: '/identities', labelKey: 'sidebar.identities', icon: UserCircle2 },
    ],
  },
  {
    labelKey: 'sidebar.group.ssh',
    items: [
      { to: '/config', labelKey: 'sidebar.sshConfig', icon: FileCode2 },
      { to: '/history', labelKey: 'sidebar.history', icon: History },
    ],
  },
  {
    labelKey: 'sidebar.group.app',
    items: [
      { to: '/settings', labelKey: 'sidebar.settings', icon: SettingsIcon },
    ],
  },
];

export function Sidebar() {
  const { t } = useTranslation();
  return (
    <aside className="w-60 shrink-0 m-3 mr-0 flex flex-col">
      <div className="flex-1 flex flex-col gap-1 rounded-2xl border border-border bg-bg-1 p-3 shadow-card overflow-y-auto">
        <div className="px-2 py-2 mb-1 text-text-0 text-[15px] font-extrabold tracking-normal">
          {t('app.name')}
        </div>
        {groups.map((group) => (
          <div key={group.labelKey} className="mt-2 first:mt-0">
            <div className="px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-text-2">
              {t(group.labelKey)}
            </div>
            <div className="flex flex-col gap-0.5 mt-0.5">
              {group.items.map((item) => (
                <NavLink
                  key={item.to}
                  to={item.to}
                  className={({ isActive }) =>
                    cn(
                      'relative flex items-center gap-2.5 px-3 py-2 rounded-md text-sm font-semibold transition-all duration-200',
                      isActive
                        ? 'bg-brand-soft text-brand-strong [[data-theme=dark]_&]:text-[#93c5fd] shadow-[inset_2px_0_0_0_var(--brand)]'
                        : 'text-text-1 hover:text-text-0 hover:bg-bg-2'
                    )
                  }
                >
                  <item.icon className="h-4 w-4 shrink-0" />
                  <span>{t(item.labelKey)}</span>
                </NavLink>
              ))}
            </div>
          </div>
        ))}
      </div>
    </aside>
  );
}
