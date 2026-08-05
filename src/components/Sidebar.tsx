import { useState } from "react";
import type { ReactNode } from "react";

export type View = "library" | "torrents" | "channels";

interface SidebarProps {
  active: View;
  onSelect: (view: View) => void;
  torrentCount: number;
}

interface NavItemProps {
  label: string;
  icon: ReactNode;
  isActive: boolean;
  isCollapsed: boolean;
  badge?: number;
  onClick: () => void;
}

function NavItem({ label, icon, isActive, isCollapsed, badge, onClick }: NavItemProps) {
  return (
    <button
      onClick={onClick}
      title={label}
      className={`flex w-full items-center rounded-lg text-xs font-medium transition-colors ${
        isCollapsed ? "justify-center p-2" : "justify-between px-3 py-2"
      } ${
        isActive
          ? "bg-zinc-200/70 text-zinc-900 dark:bg-zinc-800 dark:text-white"
          : "text-zinc-600 hover:bg-zinc-200/40 dark:text-zinc-400 dark:hover:bg-zinc-800/50"
      }`}
    >
      <span className="flex items-center gap-2.5">
        <span className={isActive ? "text-sky-600 dark:text-sky-400" : "opacity-70"}>{icon}</span>
        {!isCollapsed && <span>{label}</span>}
      </span>
      {!isCollapsed && badge !== undefined && badge > 0 && (
        <span className="rounded-full bg-sky-600/15 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-sky-600 dark:text-sky-400">
          {badge}
        </span>
      )}
    </button>
  );
}

// Iconos propios, sin librería externa — evita traer lucide-react para dos
// trazos simples y mantiene el bundle liviano (coherente con "lo más
// liviano posible" del brief original).
const LibraryIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" className="h-4 w-4">
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 9h18M9 9v11" />
  </svg>
);

const TorrentIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" className="h-4 w-4">
    <path d="M12 3v12m0 0-4-4m4 4 4-4M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />
  </svg>
);

const ChannelsIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" className="h-4 w-4">
    <path d="M4 15a8 8 0 0 1 16 0M7.5 15a4.5 4.5 0 0 1 9 0" />
    <circle cx="12" cy="15" r="1.25" fill="currentColor" stroke="none" />
  </svg>
);

export function Sidebar({ active, onSelect, torrentCount }: SidebarProps) {
  const [isCollapsed, setIsCollapsed] = useState(false);

  return (
    <aside
      className={`flex shrink-0 flex-col border-r border-zinc-200 bg-white transition-all duration-200 dark:border-zinc-800 dark:bg-zinc-950 ${
        isCollapsed ? "w-14" : "w-56"
      }`}
    >
      <div className="flex items-center justify-between border-b border-zinc-200 p-3 dark:border-zinc-800">
        {!isCollapsed && (
          <span className="text-xs font-bold tracking-tight text-zinc-900 dark:text-white">
            Popcorn
          </span>
        )}
        <button
          onClick={() => setIsCollapsed((c) => !c)}
          className="rounded-md p-1 text-zinc-500 hover:bg-zinc-200/60 dark:hover:bg-zinc-800"
          title={isCollapsed ? "Expandir" : "Colapsar"}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
            {isCollapsed ? <path d="m9 18 6-6-6-6" /> : <path d="m15 18-6-6 6-6" />}
          </svg>
        </button>
      </div>

      <nav className="flex flex-1 flex-col gap-1 p-2">
        <NavItem
          label="Colección"
          icon={<LibraryIcon />}
          isActive={active === "library"}
          isCollapsed={isCollapsed}
          onClick={() => onSelect("library")}
        />
        <NavItem
          label="Torrents"
          icon={<TorrentIcon />}
          isActive={active === "torrents"}
          isCollapsed={isCollapsed}
          badge={torrentCount}
          onClick={() => onSelect("torrents")}
        />
        <NavItem
          label="Canales"
          icon={<ChannelsIcon />}
          isActive={active === "channels"}
          isCollapsed={isCollapsed}
          onClick={() => onSelect("channels")}
        />
      </nav>
    </aside>
  );
}
