import { api } from "../lib/api";
import type { TorrentInfo } from "../types";

interface TorrentListProps {
  torrents: TorrentInfo[];
  onChanged: () => void;
}

function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} KB`;
  return `${n} B`;
}

const STATE_LABEL: Record<TorrentInfo["state"], string> = {
  initializing: "Iniciando…",
  live: "Descargando",
  paused: "Pausado",
  error: "Error",
};

export function TorrentList({ torrents, onChanged }: TorrentListProps) {
  if (torrents.length === 0) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
        No hay torrents activos todavía.
      </p>
    );
  }

  return (
    <ul className="flex flex-col gap-2 p-4">
      {torrents.map((t) => {
        const pct = t.total_bytes > 0 ? (t.progress_bytes / t.total_bytes) * 100 : 0;
        return (
          <li
            key={t.id}
            className="flex flex-col gap-2 rounded-xl border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900"
          >
            <div className="flex items-center justify-between gap-2">
              <p className="min-w-0 truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">
                {t.name ?? t.info_hash}
              </p>
              <div className="flex shrink-0 items-center gap-1.5">
                <button
                  onClick={() => api.pauseTorrent(t.id).then(onChanged)}
                  className="rounded-md border border-zinc-300 px-2 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
                >
                  {t.state === "paused" ? "Reanudar" : "Pausar"}
                </button>
                <button
                  onClick={() => api.removeTorrent(t.id, false).then(onChanged)}
                  className="rounded-md border border-red-300 px-2 py-1 text-[11px] text-red-600 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40"
                >
                  Quitar
                </button>
              </div>
            </div>

            <div className="h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-800">
              <div
                className="h-full bg-sky-600 transition-all"
                style={{ width: `${Math.min(pct, 100)}%` }}
              />
            </div>

            <div className="flex items-center justify-between font-mono text-[10px] text-zinc-500 dark:text-zinc-400">
              <span>
                {formatBytes(t.progress_bytes)} / {formatBytes(t.total_bytes)} ({pct.toFixed(1)}%)
              </span>
              <span>{STATE_LABEL[t.state]}</span>
              <span>
                ↓ {t.download_speed_mbps.toFixed(2)} MB/s · ↑ {t.upload_speed_mbps.toFixed(2)} MB/s
              </span>
            </div>
            {t.error && <p className="text-[10px] text-red-500">{t.error}</p>}
          </li>
        );
      })}
    </ul>
  );
}
