import { useEffect, useRef, useState } from "react";
import Hls from "hls.js";
import { api } from "../lib/api";
import type { MediaItem } from "../types";

type VideoPlayerProps =
  | { kind: "media"; item: MediaItem; onClose: () => void }
  | { kind: "channel"; title: string; url: string; sourceId: string | null; onClose: () => void }
  | { kind: "local"; path: string; name: string; onClose: () => void }
  | { kind: "online"; title: string; url: string; onClose: () => void }
  | { kind: "recording"; id: string; name: string; durationSeconds: number; onClose: () => void };

const DEFAULT_MAX_RECORDING_MINUTES = 180;

function formatElapsed(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function VideoPlayer(props: VideoPlayerProps) {
  const { onClose, kind } = props;
  const title =
    props.kind === "media" ? props.item.title
    : props.kind === "local" || props.kind === "recording" ? props.name
    : props.title;
  const mediaItemId = props.kind === "media" ? props.item.id : null;
  const channelUrl = props.kind === "channel" ? props.url : null;
  const channelSourceId = props.kind === "channel" ? props.sourceId : null;
  const localPath = props.kind === "local" ? props.path : null;
  const onlineUrl = props.kind === "online" ? props.url : null;
  const playbackRecordingId = props.kind === "recording" ? props.id : null;
  const recordingDurationSeconds = props.kind === "recording" ? props.durationSeconds : null;

  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);

  // Solo tiene sentido para contenido no-vivo con identidad estable entre
  // aperturas — "channel" es TV en vivo, "online" es la URL efímera del
  // primer play (la próxima apertura ya es "media", ver OnlineLibraryTab).
  const resumeKey =
    kind === "media" ? `popcorn.playbackPosition.media:${mediaItemId}`
    : kind === "local" ? `popcorn.playbackPosition.local:${localPath}`
    : kind === "recording" ? `popcorn.playbackPosition.recording:${playbackRecordingId}`
    : null;

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
    } else if (kind === "online" && onlineUrl) {
      // Ya viene resuelta por el caller (OnlineLibraryTab: addOnlineItem +
      // getStreamUrl) — sin fetch adicional acá, mismo criterio que "channel".
      setUrl(onlineUrl);
    } else if (kind === "recording" && playbackRecordingId) {
      api.getRecordingStreamUrl(playbackRecordingId).then(setUrl).catch((e) => setError(String(e)));
    }
  }, [kind, mediaItemId, channelUrl, localPath, onlineUrl, playbackRecordingId]);

  useEffect(() => {
    if ((kind !== "channel" && kind !== "recording") || !url) return;
    const video = videoRef.current;
    if (!video) return;

    // "recording" es .ts crudo (concatenación real de segmentos HLS, ver
    // iptv/recorder.rs) — <video src> directo no lo reproduce en la
    // mayoría de navegadores (MEDIA_ERR_SRC_NOT_SUPPORTED, confirmado en
    // vivo). hls.js sí trae demuxer de MPEG-TS, pero espera un manifest —
    // se arma uno sintético de un solo segmento apuntando al mismo
    // archivo, reusando el demuxer en vez de duplicar lógica de remux.
    // La duración tiene que ser la real (started_at/stopped_at, ver
    // IptvView) — un placeholder inventado confundió el buffering de
    // hls.js (pantalla negra sin error, confirmado en vivo).
    const objectUrls: string[] = [];
    const source =
      kind === "recording"
        ? (() => {
            const duration = Math.ceil(recordingDurationSeconds ?? 60);
            const manifest = `#EXTM3U\n#EXT-X-TARGETDURATION:${duration}\n#EXTINF:${duration},\n${url}\n#EXT-X-ENDLIST\n`;
            const blobUrl = URL.createObjectURL(new Blob([manifest], { type: "application/vnd.apple.mpegurl" }));
            objectUrls.push(blobUrl);
            return blobUrl;
          })()
        : url;

    if (Hls.isSupported()) {
      const hls = new Hls();
      hls.loadSource(source);
      hls.attachMedia(video);
      hls.on(Hls.Events.ERROR, (_event, data) => {
        // Mismo espíritu que el MediaError real del bug #18: no tragarse el
        // motivo — hls.js distingue tipo/detalle/si es fatal.
        console.error(`[popcorn] hls.js error: type=${data.type} details=${data.details} fatal=${data.fatal}`);
        if (data.fatal) {
          setError(`No se pudo reproducir (hls.js: ${data.type}/${data.details}).`);
        } else if (kind === "recording") {
          // Sin acceso confiable a devtools para diagnosticar "recording"
          // (feature nueva, frágil) — se muestra igual aunque no sea fatal,
          // temporal hasta confirmar que anda de punta a punta.
          setError((prev) => `${prev ? prev + " | " : ""}(no fatal) ${data.type}/${data.details}`);
        }
      });
      if (kind === "recording") {
        hls.on(Hls.Events.MANIFEST_PARSED, (_e, data) => {
          console.error(`[popcorn] hls.js manifest parsed: levels=${data.levels.length}`);
          // autoPlay (atributo HTML) puede fallar en silencio con MSE en
          // este WebView — .play() explícito rechaza una promesa real que
          // sí podemos atrapar y mostrar, en vez de quedar bufferizando
          // para siempre sin que nadie consuma el buffer (bufferFullError,
          // confirmado en vivo).
          video.play().catch((e) => {
            console.error(`[popcorn] video.play() rechazado: ${e}`);
            setError(`No se pudo iniciar la reproducción automáticamente: ${e}`);
          });
        });
        hls.on(Hls.Events.FRAG_LOADED, (_e, data) => {
          console.error(`[popcorn] hls.js frag loaded: bytes=${data.frag.stats?.total ?? "?"}`);
        });
      }
      return () => {
        hls.destroy();
        objectUrls.forEach((u) => URL.revokeObjectURL(u));
      };
    }
    if (video.canPlayType("application/vnd.apple.mpegurl")) {
      video.src = source;
      return;
    }
    setError("Este navegador no soporta HLS.");
  }, [kind, url, recordingDurationSeconds]);

  const lastSavedRef = useRef(0);

  function saveCurrentPosition() {
    const video = videoRef.current;
    if (!resumeKey || !video) return;
    localStorage.setItem(resumeKey, String(video.currentTime));
  }

  // REC vive en el reproductor, no en la lista de canales (estilo
  // videocasetera: grabás lo que estás viendo) — solo aplica a "channel".
  const [recordingId, setRecordingId] = useState<string | null>(null);
  const [recordingSeconds, setRecordingSeconds] = useState(0);
  const [showDurationPrompt, setShowDurationPrompt] = useState(false);
  const [maxDurationMinutes, setMaxDurationMinutes] = useState(DEFAULT_MAX_RECORDING_MINUTES);
  const [recordingError, setRecordingError] = useState<string | null>(null);

  useEffect(() => {
    if (!recordingId) return;
    const id = setInterval(() => setRecordingSeconds((s) => s + 1), 1000);
    return () => clearInterval(id);
  }, [recordingId]);

  async function startRecording() {
    if (kind !== "channel" || !channelUrl) return;
    setShowDurationPrompt(false);
    setRecordingError(null);
    try {
      const info = await api.startRecording(channelSourceId, title, channelUrl, maxDurationMinutes);
      setRecordingId(info.id);
      setRecordingSeconds(0);
    } catch (e) {
      setRecordingError(`No se pudo grabar: ${e}`);
    }
  }

  async function stopRecording() {
    if (!recordingId) return;
    const id = recordingId;
    setRecordingId(null);
    await api.stopRecording(id).catch((e) => console.error(`[popcorn] no se pudo detener la grabación ${id}: ${e}`));
  }

  function handleClose() {
    saveCurrentPosition();
    // Dejás de ver, deja de grabar — no queda una grabación huérfana en
    // background sin que el usuario la vea en pantalla.
    if (recordingId) {
      api.stopRecording(recordingId).catch((e) => console.error(`[popcorn] no se pudo detener la grabación al cerrar: ${e}`));
    }
    onClose();
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4">
      <div className="flex w-full max-w-4xl flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <h2 className="min-w-0 truncate text-sm font-medium text-white">{title}</h2>
          <div className="flex shrink-0 items-center gap-2">
            {kind === "channel" && !recordingId && !showDurationPrompt && (
              <button
                onClick={() => setShowDurationPrompt(true)}
                className="flex items-center gap-1.5 rounded-md border border-red-500/60 px-2 py-1 text-[11px] font-semibold text-red-400 hover:bg-red-500/10"
              >
                <span className="h-2 w-2 rounded-full bg-red-500" />
                REC
              </button>
            )}
            {kind === "channel" && showDurationPrompt && (
              <div className="flex items-center gap-1.5 rounded-md border border-zinc-700 bg-zinc-900 px-2 py-1">
                <label className="text-[10px] text-zinc-400">Máx (min)</label>
                <input
                  type="number"
                  min={1}
                  value={maxDurationMinutes}
                  onChange={(e) => setMaxDurationMinutes(Number(e.target.value) || DEFAULT_MAX_RECORDING_MINUTES)}
                  className="w-14 rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 text-[11px] text-white"
                />
                <button
                  onClick={startRecording}
                  className="rounded bg-red-600 px-2 py-0.5 text-[11px] font-semibold text-white hover:bg-red-500"
                >
                  Iniciar
                </button>
                <button
                  onClick={() => setShowDurationPrompt(false)}
                  className="text-[11px] text-zinc-400 hover:text-zinc-200"
                >
                  Cancelar
                </button>
              </div>
            )}
            {kind === "channel" && recordingId && (
              <button
                onClick={stopRecording}
                className="flex items-center gap-1.5 rounded-md border border-red-500 bg-red-500/10 px-2 py-1 text-[11px] font-semibold text-red-400"
              >
                <span className="h-2 w-2 animate-pulse rounded-full bg-red-500" />
                {formatElapsed(recordingSeconds)} · Detener
              </button>
            )}
            <button
              onClick={handleClose}
              className="rounded-md p-1.5 text-zinc-300 hover:bg-white/10"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-5 w-5">
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
          </div>
        </div>
        {recordingError && <p className="text-xs text-red-400">{recordingError}</p>}

        <div className="flex aspect-video items-center justify-center overflow-hidden rounded-xl bg-black">
          {error && <p className="p-4 text-center text-xs text-red-400">{error}</p>}
          {!error && !url && (
            <p className="text-xs text-zinc-400">Resolviendo fuente de streaming…</p>
          )}
          {url && (
            <video
              ref={videoRef}
              src={kind !== "channel" && kind !== "recording" ? url : undefined}
              controls
              autoPlay
              className="h-full w-full"
              onLoadedMetadata={() => {
                if (!resumeKey) return;
                const video = videoRef.current;
                if (!video) return;
                const saved = Number(localStorage.getItem(resumeKey));
                // No retoma si está a menos de 15s del final — evita reabrir
                // justo en los créditos de algo que ya se terminó de ver.
                if (saved > 0 && saved < video.duration - 15) {
                  video.currentTime = saved;
                }
              }}
              onTimeUpdate={() => {
                const video = videoRef.current;
                if (!resumeKey || !video) return;
                if (Math.abs(video.currentTime - lastSavedRef.current) < 5) return;
                lastSavedRef.current = video.currentTime;
                saveCurrentPosition();
              }}
              onEnded={() => {
                if (resumeKey) localStorage.removeItem(resumeKey);
              }}
              onError={() => {
                if (kind === "channel" || kind === "recording") return; // hls.js ya reporta sus propios errores, más específicos
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
