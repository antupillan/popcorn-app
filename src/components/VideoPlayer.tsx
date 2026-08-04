import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { MediaItem } from "../types";

interface VideoPlayerProps {
  item: MediaItem;
  onClose: () => void;
}

export function VideoPlayer({ item, onClose }: VideoPlayerProps) {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    setUrl(null);
    setError(null);
    // Vía media_items.id, no engine_torrent_id directo: el backend sana el
    // torrent si la sesión del motor lo perdió al reiniciar la app (bug #26).
    api
      .getStreamUrlForMediaItem(item.id, 0)
      .then(setUrl)
      .catch((e) => setError(String(e)));
  }, [item]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4">
      <div className="flex w-full max-w-4xl flex-col gap-2">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-white">{item.title}</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1.5 text-zinc-300 hover:bg-white/10"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-5 w-5">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="flex aspect-video items-center justify-center overflow-hidden rounded-xl bg-black">
          {error && <p className="p-4 text-center text-xs text-red-400">{error}</p>}
          {!error && !url && (
            <p className="text-xs text-zinc-400">Resolviendo fuente de streaming…</p>
          )}
          {url && (
            <video
              ref={videoRef}
              src={url}
              controls
              autoPlay
              className="h-full w-full"
              onError={() => {
                const el = videoRef.current;
                const mediaError = el?.error;
                // El código/mensaje de MediaError es la señal real que falta
                // hoy para diagnosticar el bug #18 (ver plan) — sin esto el
                // navegador descarta el motivo real (red/decode/formato) y
                // solo queda un fallo genérico, sin poder distinguir causa.
                const detail = mediaError
                  ? `code=${mediaError.code} message="${mediaError.message || "(el navegador no dio mensaje)"}"`
                  : "sin MediaError disponible";
                const position = el
                  ? `${el.currentTime.toFixed(1)}s/${el.duration ? el.duration.toFixed(1) : "?"}s`
                  : "posición desconocida";
                console.error(`[popcorn] video playback error: ${detail} en ${position}`);
                setError(`El reproductor no pudo cargar el stream (${detail}, en ${position}).`);
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
