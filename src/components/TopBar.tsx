interface TopBarProps {
  downloadSpeedMbps: number;
  uploadSpeedMbps: number;
  isDark: boolean;
  onToggleTheme: () => void;
  onOpenAddTorrent: () => void;
}

function formatSpeed(mbps: number): string {
  if (mbps >= 1) return `${mbps.toFixed(1)} MB/s`;
  return `${(mbps * 1024).toFixed(0)} KB/s`;
}

export function TopBar({
  downloadSpeedMbps,
  uploadSpeedMbps,
  isDark,
  onToggleTheme,
  onOpenAddTorrent,
}: TopBarProps) {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b border-zinc-200 bg-white/80 px-4 backdrop-blur-md dark:border-zinc-800 dark:bg-zinc-950/80">
      <div className="flex items-center gap-4 font-mono text-xs">
        <span className="flex items-center gap-1.5 text-sky-600 dark:text-sky-400">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-3.5 w-3.5">
            <path d="M12 19V5m0 14-5-5m5 5 5-5" />
          </svg>
          {formatSpeed(downloadSpeedMbps)}
        </span>
        <span className="flex items-center gap-1.5 text-zinc-500 dark:text-zinc-400">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-3.5 w-3.5">
            <path d="M12 5v14m0-14 5 5m-5-5-5 5" />
          </svg>
          {formatSpeed(uploadSpeedMbps)}
        </span>
      </div>

      <div className="flex items-center gap-2">
        <button
          onClick={onToggleTheme}
          className="rounded-lg border border-zinc-200 p-2 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-900"
          title={isDark ? "Cambiar a modo claro" : "Cambiar a modo oscuro"}
        >
          {isDark ? (
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <circle cx="12" cy="12" r="4" />
              <path d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32 1.41 1.41M2 12h2m16 0h2M4.93 19.07l1.41-1.41m11.32-11.32 1.41-1.41" />
            </svg>
          ) : (
            <svg viewBox="0 0 24 24" fill="currentColor" className="h-4 w-4">
              <path d="M20.354 15.354A9 9 0 0 1 8.646 3.646 9.003 9.003 0 1 0 20.354 15.354Z" />
            </svg>
          )}
        </button>

        <button
          onClick={onOpenAddTorrent}
          className="flex items-center gap-1.5 rounded-lg bg-sky-600 px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-sky-500"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" className="h-3.5 w-3.5">
            <path d="M12 5v14m-7-7h14" />
          </svg>
          Agregar Torrent
        </button>
      </div>
    </header>
  );
}
