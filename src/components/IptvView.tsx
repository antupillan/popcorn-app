import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { Channel, IptvSource, RecordingInfo } from "../types";
import { AddIptvSourceModal } from "./AddIptvSourceModal";

type Tab = "channels" | "sources" | "recordings";

const TABS: { id: Tab; label: string }[] = [
  { id: "channels", label: "Canales" },
  { id: "sources", label: "Fuentes" },
  { id: "recordings", label: "Grabaciones" },
];

interface IptvViewProps {
  onPlayChannel: (channel: { title: string; url: string }) => void;
}

export function IptvView({ onPlayChannel }: IptvViewProps) {
  const [tab, setTab] = useState<Tab>("channels");

  return (
    <div>
      {/* `main` (App.tsx) ya provee el scroll — este tab bar no queda fijo
          arriba al hacer scroll, mismo comportamiento simple que el resto
          de las vistas (MediaLibrary/TorrentList tampoco tienen sub-nav
          pegajoso). */}
      <div className="sticky top-0 z-10 flex gap-1 border-b border-zinc-200 bg-slate-50 px-4 pt-2 dark:border-zinc-800 dark:bg-zinc-950">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`rounded-t-md px-3 py-1.5 text-xs font-medium transition-colors ${
              tab === t.id
                ? "border-b-2 border-sky-600 text-sky-600 dark:text-sky-400"
                : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "channels" && <ChannelsTab onPlayChannel={onPlayChannel} />}
      {tab === "sources" && <SourcesTab />}
      {tab === "recordings" && <RecordingsTab />}
    </div>
  );
}

function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} KB`;
  return `${n} B`;
}

function ChannelsTab({ onPlayChannel }: IptvViewProps) {
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyUrl, setBusyUrl] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setLoading(true);
    api
      .listChannels()
      .then(setChannels)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  async function play(channel: Channel) {
    setBusyUrl(channel.url);
    setMessage(null);
    try {
      const validUrl = await api.validateChannelManifest(channel.url);
      onPlayChannel({ title: channel.name, url: validUrl });
    } catch (e) {
      setMessage(`No se pudo validar el canal: ${e}`);
    } finally {
      setBusyUrl(null);
    }
  }

  if (loading) {
    return <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">Cargando canales…</p>;
  }
  if (error) {
    return <p className="p-6 text-center text-xs text-red-500">{error}</p>;
  }
  if (channels.length === 0) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
        No hay canales — agregá una fuente en la pestaña "Fuentes".
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-2 p-4">
      {message && <p className="text-xs text-sky-600 dark:text-sky-400">{message}</p>}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        {channels.map((c) => {
          const key = `${c.source_id}:${c.url}`;
          return (
            <button
              key={key}
              onClick={() => play(c)}
              disabled={busyUrl === c.url}
              className="group flex flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white text-left disabled:cursor-wait dark:border-zinc-800 dark:bg-zinc-900"
            >
              <div className="relative flex aspect-video items-center justify-center bg-zinc-100 dark:bg-zinc-800">
                {c.logo_url ? (
                  <img
                    src={c.logo_url}
                    alt=""
                    className="h-full w-full object-contain p-3"
                    loading="lazy"
                    onError={(e) => (e.currentTarget.style.visibility = "hidden")}
                  />
                ) : (
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.5"
                    className="h-8 w-8 text-zinc-400 opacity-50"
                  >
                    <path d="M4 15a8 8 0 0 1 16 0M7.5 15a4.5 4.5 0 0 1 9 0" />
                    <circle cx="12" cy="15" r="1.25" fill="currentColor" stroke="none" />
                  </svg>
                )}
                <span className="absolute inset-0 flex items-center justify-center bg-black/0 transition-colors group-hover:bg-black/25">
                  {busyUrl === c.url ? (
                    <svg viewBox="0 0 24 24" className="h-9 w-9 animate-spin text-white/90 drop-shadow">
                      <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeWidth="2.5" strokeDasharray="42" strokeDashoffset="14" strokeLinecap="round" />
                    </svg>
                  ) : (
                    <svg viewBox="0 0 24 24" className="h-9 w-9 text-white/80 opacity-0 drop-shadow transition-opacity group-hover:opacity-100">
                      <path d="M8 5v14l11-7z" fill="currentColor" />
                    </svg>
                  )}
                </span>
              </div>
              <div className="p-2">
                <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{c.name}</p>
                {c.group && <p className="truncate text-[10px] text-zinc-500">{c.group}</p>}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function SourcesTab() {
  const [sources, setSources] = useState<IptvSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);

  function refresh() {
    setLoading(true);
    api
      .listIptvSources()
      .then(setSources)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }

  useEffect(refresh, []);

  return (
    <div className="flex flex-col gap-3 p-4">
      <button
        onClick={() => setModalOpen(true)}
        className="self-start rounded-lg bg-sky-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-sky-500"
      >
        + Agregar fuente
      </button>

      {loading && <p className="text-xs text-zinc-500 dark:text-zinc-400">Cargando fuentes…</p>}
      {error && <p className="text-xs text-red-500">{error}</p>}

      <ul className="flex flex-col gap-1.5">
        {sources.map((s) => (
          <li
            key={s.id}
            className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 dark:border-zinc-800"
          >
            <div className="min-w-0">
              <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{s.name}</p>
              <p className="truncate text-[10px] text-zinc-500">
                {s.source_kind === "url" ? s.playlist_url : "(archivo local)"}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <button
                onClick={() => api.toggleIptvSource(s.id, !s.enabled).then(refresh)}
                className="rounded-md border border-zinc-300 px-2 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
              >
                {s.enabled ? "Desactivar" : "Activar"}
              </button>
              <button
                onClick={() => api.removeIptvSource(s.id).then(refresh)}
                className="rounded-md border border-red-300 px-2 py-1 text-[11px] text-red-600 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40"
              >
                Quitar
              </button>
            </div>
          </li>
        ))}
      </ul>

      {modalOpen && (
        <AddIptvSourceModal onClose={() => setModalOpen(false)} onAdded={refresh} />
      )}
    </div>
  );
}

const RECORDING_STATUS_LABEL: Record<RecordingInfo["status"], string> = {
  recording: "Grabando",
  stopped: "Detenida",
  error: "Error",
};

function RecordingsTab() {
  const [recordings, setRecordings] = useState<RecordingInfo[]>([]);

  useEffect(() => {
    function refresh() {
      api.listRecordings().then(setRecordings).catch(console.error);
    }
    refresh();
    // Poll mientras la pestaña está montada — bytes_written avanza en
    // segundo plano sin ninguna acción del usuario que dispare un refresh.
    const id = setInterval(refresh, 2000);
    return () => clearInterval(id);
  }, []);

  if (recordings.length === 0) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
        No hay grabaciones todavía — usá "Grabar" en la pestaña Canales.
      </p>
    );
  }

  return (
    <ul className="flex flex-col gap-2 p-4">
      {recordings.map((r) => (
        <li
          key={r.id}
          className="flex flex-col gap-1.5 rounded-xl border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900"
        >
          <div className="flex items-center justify-between gap-2">
            <p className="min-w-0 truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">
              {r.channel_name}
            </p>
            {r.status === "recording" && (
              <button
                onClick={() => api.stopRecording(r.id)}
                className="shrink-0 rounded-md border border-red-300 px-2 py-1 text-[11px] text-red-600 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40"
              >
                Detener
              </button>
            )}
          </div>
          <div className="flex items-center justify-between font-mono text-[10px] text-zinc-500 dark:text-zinc-400">
            <span>{formatBytes(r.bytes_written)}</span>
            <span>{RECORDING_STATUS_LABEL[r.status]}</span>
          </div>
          {r.error && <p className="text-[10px] text-red-500">{r.error}</p>}
        </li>
      ))}
    </ul>
  );
}
