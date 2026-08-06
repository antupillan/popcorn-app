import { useEffect, useRef, useState } from "react";
import Hls from "hls.js";
import { api } from "../lib/api";
import type { MediaItem } from "../types";

type VideoPlayerProps =
  | { kind: "media"; item: MediaItem; onClose: () => void }
  | { kind: "channel"; title: string; url: string; onClose: () => void }
  | { kind: "local"; path: string; name: string; onClose: () => void };

export function VideoPlayer(props: VideoPlayerProps) {
  const { onClose, kind } = props;
  const title = props.kind === "media" ? props.item.title : props.kind === "local" ? props.name : props.title;
  const mediaItemId = props.kind === "media" ? props.item.id : null;
  const channelUrl = props.kind === "channel" ? props.url : null;
  const localPath = props.kind === "local" ? props.path : null;

  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    setUrl(null);
    setError(null);
    if (kind === "media" && mediaItemId) {
      // Vía media_items.id, no engine_torrent_id directo: el backend sana
      // el torrent si la sesión del motor lo perdió al reiniciar la app
      // (bug #26).
      api
        .getStreamUrlForMediaItem(mediaItemId, 0)
        .then(setUrl)
        .catch((e) => setError(String(e)));
    } else if (kind === "channel" && channelUrl) {
      // La URL de canal ya viene validada por validateChannelManifest antes
      // de abrir el player ("supervisión liviana", ver plan IPTV) — acá no
      // hay resolución adicional que hacer.
      setUrl(channelUrl);
    } else if (kind === "local" && localPath) {
      api.getLocalStreamUrl(localPath).then(setUrl).catch((e) => setError(String(e)));
    }
  }, [kind, mediaItemId, channelUrl, localPath]);

  useEffect(() => {
    if (kind !== "channel" || !url) return;
    const video = videoRef.current;
    if (!video) return;

    if (Hls.isSupported()) {
      const hls = new Hls();
      hls.loadSource(url);
      hls.attachMedia(video);
      hls.on(Hls.Events.ERROR, (_event, data) => {
        // Mismo espíritu que el MediaError real del bug #18: no tragarse el
        // motivo — hls.js distingue tipo/detalle/si es fatal.
        console.error(`[popcorn] hls.js error: type=${data.type} details=${data.details} fatal=${data.fatal}`);
        if (data.fatal) {
          setError(`El canal no pudo reproducirse (hls.js: ${data.type}/${data.details}).`);
        }
      });
      return () => hls.destroy();
    }
    if (video.canPlayType("application/vnd.apple.mpegurl")) {
      video.src = url;
      return;
    }
    setError("Este navegador no soporta HLS.");
  }, [kind, url]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4">
      <div className="flex w-full max-w-4xl flex-col gap-2">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-white">{title}</h2>
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
              src={kind !== "channel" ? url : undefined}
              controls
              autoPlay
              className="h-full w-full"
              onError={() => {
                if (kind === "channel") return; // hls.js ya reporta sus propios errores, más específicos
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
