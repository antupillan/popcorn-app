import type { MediaItem } from "../types";

interface MediaLibraryProps {
  items: MediaItem[];
  onPlay: (item: MediaItem) => void;
}

export function MediaLibrary({ items, onPlay }: MediaLibraryProps) {
  if (items.length === 0) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
        Tu biblioteca está vacía — usá "Agregar Torrent" para sumar algo del catálogo legal.
      </p>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-3 p-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
      {items.map((item) => (
        <button
          key={item.id}
          onClick={() => onPlay(item)}
          className="group flex flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white text-left transition-colors hover:border-sky-500 dark:border-zinc-800 dark:bg-zinc-900"
        >
          <div className="flex aspect-video items-center justify-center bg-zinc-100 text-zinc-400 dark:bg-zinc-800">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="h-8 w-8 opacity-50 transition-opacity group-hover:opacity-100">
              <path d="m9 8 6 4-6 4V8Z" />
              <rect x="3" y="4" width="18" height="16" rx="2" />
            </svg>
          </div>
          <div className="p-2.5">
            <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">
              {item.title}
            </p>
            <p className="text-[10px] text-zinc-500">{item.year ?? "—"}</p>
          </div>
        </button>
      ))}
    </div>
  );
}
