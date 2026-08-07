import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { MediaItem, OnlineItem } from "../types";

const KIND_LABEL: Record<OnlineItem["kind"], string> = {
  archive_org: "archive.org",
  public_domain_torrents: "Public Domain Torrents",
  blender_foundation: "Blender Foundation",
  prelinger: "Prelinger Archives",
  feature_films: "Cine clásico",
};

interface OnlineLibraryTabProps {
  mediaItems: MediaItem[];
  onPlayMedia: (item: MediaItem) => void;
  onPlayOnline: (title: string, url: string) => void;
  onAdded: () => void;
}

export function OnlineLibraryTab({ mediaItems, onPlayMedia, onPlayOnline, onAdded }: OnlineLibraryTabProps) {
  const [items, setItems] = useState<OnlineItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [pdtLoading, setPdtLoading] = useState(true);
  const [pdtError, setPdtError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    setLoading(true);
    api
      .browseOnlineLibrary()
      .then(setItems)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));

    // Public Domain Torrents es mucho más lento que el resto (~8s vs ~1.5s,
    // medido en vivo — ver plan, sección "investigar lentitud") — se pide
    // por separado y se agrega apenas responde, sin bloquear el resto del
    // catálogo detrás suyo.
    setPdtLoading(true);
    api
      .browsePublicDomainTorrents()
      .then((pdtItems) => setItems((prev) => [...prev, ...pdtItems]))
      .catch((e) => setPdtError(String(e)))
      .finally(() => setPdtLoading(false));
  }, []);

  // undefined si el ítem todavía no está en la colección del usuario —
  // source_identifier guarda el identifier de OnlineItem tal cual, sin
  // transformar, para las cinco fuentes (ver add_archive_org_item_core/
  // add_online_item_inner en el backend).
  function findExisting(item: OnlineItem): MediaItem | undefined {
    return mediaItems.find((m) => m.source_identifier === item.identifier);
  }

  async function view(item: OnlineItem) {
    const existing = findExisting(item);
    if (existing) {
      onPlayMedia(existing);
      return;
    }
    const key = `${item.kind}:${item.identifier}`;
    setBusyKey(key);
    setMessage(null);
    try {
      const info = await api.addOnlineItem(item.kind, item.identifier, item.title, item.year, item.license);
      const url = await api.getStreamUrl(info.id, 0);
      onPlayOnline(item.title, url);
      onAdded();
    } catch (e) {
      setMessage(`No se pudo reproducir "${item.title}": ${e}`);
    } finally {
      setBusyKey(null);
    }
  }

  // Solo tiene sentido para la familia archive.org (Public Domain Torrents
  // ya es P2P real desde que se agrega, sin proxy de por medio) — descarga
  // el .torrent completo antes de agregarlo, puede tardar bastante más que
  // "Ver".
  async function seed(item: OnlineItem) {
    const key = `${item.kind}:${item.identifier}:seed`;
    setBusyKey(key);
    setMessage(null);
    try {
      await api.seedArchiveOrgItem(item.identifier, item.title, item.year, item.license);
      setMessage(`Sembrando de verdad: ${item.title} (ver pestaña Torrents para pausar/quitar).`);
      onAdded();
    } catch (e) {
      setMessage(`No se pudo sembrar "${item.title}": ${e}`);
    } finally {
      setBusyKey(null);
    }
  }

  if (loading) {
    return <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">Cargando catálogo…</p>;
  }
  if (error) {
    return <p className="p-6 text-center text-xs text-red-500">{error}</p>;
  }
  if (items.length === 0 && !pdtLoading) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
        No hay resultados en el catálogo Online por ahora.
      </p>
    );
  }

  const filtered = items.filter((i) => i.title.toLowerCase().includes(query.toLowerCase()));

  return (
    <div className="flex flex-col gap-2 p-4">
      <input
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Buscar por nombre…"
        className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs text-zinc-900 placeholder:text-zinc-400 focus:border-sky-500 focus:outline-none dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100"
      />
      {message && <p className="text-xs text-sky-600 dark:text-sky-400">{message}</p>}
      {filtered.length === 0 && (
        <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
          Sin resultados para "{query}".
        </p>
      )}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        {filtered.map((item) => {
          const key = `${item.kind}:${item.identifier}`;
          const existing = findExisting(item);
          return (
            <div
              key={key}
              className="relative flex flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-zinc-800 dark:bg-zinc-900"
            >
              {existing && (
                <span className="absolute left-1.5 top-1.5 z-10 rounded-full bg-emerald-600/90 px-1.5 py-0.5 text-[9px] font-semibold text-white">
                  En tu colección
                </span>
              )}
              <div className="flex aspect-video items-center justify-center bg-zinc-100 dark:bg-zinc-800">
                {item.thumbnail_url ? (
                  <img
                    src={item.thumbnail_url}
                    alt=""
                    className="h-full w-full object-cover"
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
                    <path d="m9 8 6 4-6 4V8Z" />
                    <rect x="3" y="4" width="18" height="16" rx="2" />
                  </svg>
                )}
              </div>
              <div className="flex flex-1 flex-col gap-1.5 p-2.5">
                <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{item.title}</p>
                <div className="flex items-center justify-between">
                  <span className="rounded-full bg-sky-600/10 px-1.5 py-0.5 text-[10px] font-medium text-sky-600 dark:text-sky-400">
                    {KIND_LABEL[item.kind]}
                  </span>
                  {item.year && <span className="text-[10px] text-zinc-500">{item.year}</span>}
                </div>
                <div className="mt-1 flex gap-1">
                  <button
                    onClick={() => view(item)}
                    disabled={busyKey === key || busyKey === `${key}:seed`}
                    className="flex-1 rounded-md border border-sky-600 px-2 py-1 text-[11px] font-semibold text-sky-600 hover:bg-sky-600 hover:text-white disabled:opacity-50 dark:text-sky-400"
                  >
                    {busyKey === key ? "Cargando…" : "Ver"}
                  </button>
                  {item.kind !== "public_domain_torrents" && (
                    <button
                      onClick={() => seed(item)}
                      disabled={busyKey === key || busyKey === `${key}:seed`}
                      title="Descarga completo y siembra de verdad al swarm real (tarda más que Ver)"
                      className="flex-1 rounded-md border border-emerald-600 px-2 py-1 text-[11px] font-semibold text-emerald-600 hover:bg-emerald-600 hover:text-white disabled:opacity-50 dark:text-emerald-400"
                    >
                      {busyKey === `${key}:seed` ? "Sembrando…" : "Sembrar"}
                    </button>
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>
      {pdtLoading && (
        <p className="p-3 text-center text-[11px] text-zinc-500 dark:text-zinc-400">
          Cargando Public Domain Torrents… (tarda más que el resto del catálogo)
        </p>
      )}
      {pdtError && (
        <p className="p-3 text-center text-[11px] text-red-500">
          Public Domain Torrents no respondió: {pdtError}
        </p>
      )}
    </div>
  );
}
