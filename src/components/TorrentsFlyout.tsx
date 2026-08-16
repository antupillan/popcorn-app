import { useEffect } from "react";
import { TorrentList } from "./TorrentList";
import type { MediaItem, TorrentInfo } from "../types";

interface TorrentsFlyoutProps {
  torrents: TorrentInfo[];
  onChanged: () => void;
  onClose: () => void;
  mediaItems: MediaItem[];
  onPlayMedia: (item: MediaItem) => void;
}

// Modal centrado con backdrop propio (mismo patrón que SearchModal/
// AddSourceModal) — antes era un dropdown anclado al HUD del TopBar con
// `position: absolute`, que quedaba tapado a medias por la cinta sticky
// de Biblioteca y por las cards del grid (bug real reportado en vivo,
// mismo tipo de problema de stacking ya visto con la cinta vs. el badge
// "En tu colección"). `position: fixed` + backdrop propio evita depender
// de en qué contenedor quede anidado.
export function TorrentsFlyout({ torrents, onChanged, onClose, mediaItems, onPlayMedia }: TorrentsFlyoutProps) {
  function playMediaAndClose(item: MediaItem) {
    onPlayMedia(item);
    onClose();
  }
  useEffect(() => {
    function onEsc(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("keydown", onEsc);
    return () => document.removeEventListener("keydown", onEsc);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[70vh] w-full max-w-lg flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-2xl dark:border-zinc-800 dark:bg-zinc-900"
      >
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">Torrents activos</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
        <div className="overflow-y-auto">
          <TorrentList torrents={torrents} onChanged={onChanged} mediaItems={mediaItems} onPlayMedia={playMediaAndClose} />
        </div>
      </div>
    </div>
  );
}
