import { useState } from "react";
import type { ReactNode } from "react";

// Torrents pasó a flyout del TopBar y Ajustes a panel flotante (ninguno de
// los dos es una "vista" que reemplace <main>). "subtitulos" ocupa el punto
// de extensión que este comentario ya preveía para una vista social — Fase 3
// (Comunidades) se pospuso, ver primer_plan_mejora.txt "[2026-08-15]
// Decisión de secuenciación".
export type View = "biblioteca" | "subtitulos";

interface SidebarProps {
  active: View;
  onSelect: (view: View) => void;
  onOpenAjustes: () => void;
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
        <span className={isActive ? "text-[var(--accent)] dark:text-[var(--accent-fg)]" : "opacity-70"}>{icon}</span>
        {!isCollapsed && <span>{label}</span>}
      </span>
      {!isCollapsed && badge !== undefined && badge > 0 && (
        <span className="rounded-full bg-[var(--accent-soft)] px-1.5 py-0.5 font-mono text-[10px] font-semibold text-[var(--accent)] dark:text-[var(--accent-fg)]">
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

const SubtitlesIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" className="h-4 w-4">
    <rect x="3" y="5" width="18" height="14" rx="2" />
    <path d="M7 14h4M13 14h4M7 10h10" />
  </svg>
);

const SettingsIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" className="h-4 w-4">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" />
  </svg>
);

export function Sidebar({ active, onSelect, onOpenAjustes }: SidebarProps) {
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
          label="Biblioteca"
          icon={<LibraryIcon />}
          isActive={active === "biblioteca"}
          isCollapsed={isCollapsed}
          onClick={() => onSelect("biblioteca")}
        />
        <NavItem
          label="Subtítulos"
          icon={<SubtitlesIcon />}
          isActive={active === "subtitulos"}
          isCollapsed={isCollapsed}
          onClick={() => onSelect("subtitulos")}
        />
      </nav>

      <div className="border-t border-zinc-200 p-2 dark:border-zinc-800">
        <NavItem
          label="Ajustes"
          icon={<SettingsIcon />}
          isActive={false}
          isCollapsed={isCollapsed}
          onClick={onOpenAjustes}
        />
      </div>
    </aside>
  );
}
