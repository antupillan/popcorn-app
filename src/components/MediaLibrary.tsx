import { useState } from "react";
import { api } from "../lib/api";
import type { MediaItem } from "../types";

interface MediaLibraryProps {
  items: MediaItem[];
  onPlay: (item: MediaItem) => void;
  onRemoved: () => void;
}

export function MediaLibrary({ items, onPlay, onRemoved }: MediaLibraryProps) {
  const [busyId, setBusyId] = useState<string | null>(null);

  async function remove(id: string) {
    setBusyId(id);
    try {
      await api.removeMediaItem(id);
      onRemoved();
    } catch (e) {
      console.error(`[popcorn] no se pudo quitar el ítem ${id}: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  if (items.length === 0) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
        Tu biblioteca está vacía — usa "Agregar Torrent" para sumar algo del catálogo legal.
      </p>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-3 p-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
      {items.map((item) => (
        <div
          key={item.id}
          className="group flex flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white transition-colors hover:border-[var(--accent-hover)] dark:border-zinc-800 dark:bg-zinc-900"
        >
          <div className="relative aspect-video bg-zinc-100 dark:bg-zinc-800">
            <button
              onClick={() => onPlay(item)}
              aria-label={`Reproducir ${item.title}`}
              className="absolute inset-0 flex items-center justify-center bg-black/0 transition-colors group-hover:bg-black/20"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                className="h-8 w-8 text-zinc-400 opacity-50 transition-opacity group-hover:opacity-100"
              >
                <path d="m9 8 6 4-6 4V8Z" />
                <rect x="3" y="4" width="18" height="16" rx="2" />
              </svg>
            </button>
            <button
              onClick={(e) => {
                e.stopPropagation();
                remove(item.id);
              }}
              disabled={busyId === item.id}
              aria-label="Quitar de la colección"
              className="absolute right-1.5 top-1.5 rounded-full bg-black/50 p-1.5 text-white/90 backdrop-blur-sm transition-colors hover:bg-red-600 disabled:opacity-50"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-3.5 w-3.5">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M6 7h12M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2m-8 0 .8 12.2A2 2 0 0 0 8.8 21h6.4a2 2 0 0 0 2-1.8L18 7"
                />
              </svg>
            </button>
          </div>
          <div className="p-2.5">
            <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{item.title}</p>
            <p className="text-[10px] text-zinc-500">{item.year ?? "—"}</p>
          </div>
        </div>
      ))}
    </div>
  );
}
