import { useEffect, useRef } from "react";
import { TorrentList } from "./TorrentList";
import type { TorrentInfo } from "../types";

interface TorrentsFlyoutProps {
  torrents: TorrentInfo[];
  onChanged: () => void;
  onClose: () => void;
}

// Anclado al HUD de velocidad del TopBar (ver requerimiento real de
// frontend, línea 182 del plan) — reemplaza el ítem completo de Sidebar
// que tenía antes, coherente con que esto es estado en vivo, no navegación.
export function TorrentsFlyout({ torrents, onChanged, onClose }: TorrentsFlyoutProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function onOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    }
    function onEsc(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("mousedown", onOutside);
    document.addEventListener("keydown", onEsc);
    return () => {
      document.removeEventListener("mousedown", onOutside);
      document.removeEventListener("keydown", onEsc);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      className="absolute left-0 top-full z-40 mt-2 w-96 max-h-[70vh] overflow-y-auto rounded-xl border border-zinc-200/50 bg-white/90 shadow-2xl backdrop-blur-xl dark:border-zinc-800/50 dark:bg-zinc-900/90"
    >
      <TorrentList torrents={torrents} onChanged={onChanged} />
    </div>
  );
}
