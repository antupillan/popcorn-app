import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { SourceSettings, YoutubeSource, YoutubeVideo } from "../types";

type Tab = "videos" | "sources";

const TABS: { id: Tab; label: string }[] = [
  { id: "videos", label: "Videos" },
  { id: "sources", label: "Fuentes" },
];

type CategoryFilter = "todos" | "cine" | "series" | "anime" | "musica";

const CATEGORY_FILTERS: { id: CategoryFilter; label: string }[] = [
  { id: "todos", label: "Todos" },
  { id: "cine", label: "Cine" },
  { id: "series", label: "Series" },
  { id: "anime", label: "Anime" },
  { id: "musica", label: "Música" },
];

interface YoutubeViewProps {
  onPlayVideo: (video: { videoId: string; title: string }) => void;
}

export function YoutubeView({ onPlayVideo }: YoutubeViewProps) {
  const [tab, setTab] = useState<Tab>("videos");

  return (
    <div>
      <div className="sticky top-0 z-10 flex gap-1 border-b border-zinc-200 bg-slate-50 px-4 pt-2 dark:border-zinc-800 dark:bg-zinc-950">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`rounded-t-md px-3 py-1.5 text-xs font-medium transition-colors ${
              tab === t.id
                ? "border-b-2 border-[var(--accent)] text-[var(--accent)] dark:text-[var(--accent-fg)]"
                : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "videos" && <VideosTab onPlayVideo={onPlayVideo} />}
      {tab === "sources" && <SourcesTab />}
    </div>
  );
}

function VideosTab({ onPlayVideo }: YoutubeViewProps) {
  const [videos, setVideos] = useState<YoutubeVideo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>("todos");

  useEffect(() => {
    setLoading(true);
    api
      .listYoutubeVideos()
      .then((fast) => {
        setVideos(fast);
        // Curación en segundo plano, nunca antes del render rápido — mismo
        // criterio que ChannelsTab/OnlineLibraryTab.
        api.curateYoutubeVideos(fast).then(setVideos).catch(() => {});
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  const filtered = categoryFilter === "todos" ? videos : videos.filter((v) => v.category === categoryFilter);

  if (loading) {
    return <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">Cargando videos…</p>;
  }
  if (error) {
    return <p className="p-6 text-center text-xs text-red-500">{error}</p>;
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex gap-1.5">
        {CATEGORY_FILTERS.map((c) => (
          <button
            key={c.id}
            onClick={() => setCategoryFilter(c.id)}
            className={`rounded-md border px-2.5 py-1 text-[11px] font-medium ${
              categoryFilter === c.id
                ? "border-[var(--accent)] bg-[var(--accent)] text-white"
                : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
            }`}
          >
            {c.label}
          </button>
        ))}
      </div>

      {videos.length === 0 && (
        <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
          No hay videos — agrega un canal en la pestaña "Fuentes".
        </p>
      )}
      {videos.length > 0 && filtered.length === 0 && (
        <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
          Ningún video en esta categoría todavía.
        </p>
      )}

      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        {filtered.map((v) => {
          const key = `${v.source_id}:${v.video_id}`;
          return (
            <button
              key={key}
              onClick={() => onPlayVideo({ videoId: v.video_id, title: v.title })}
              className="group flex flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white text-left dark:border-zinc-800 dark:bg-zinc-900"
            >
              <div className="relative flex aspect-video items-center justify-center bg-zinc-100 dark:bg-zinc-800">
                {v.thumbnail_url ? (
                  <img
                    src={v.thumbnail_url}
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
                    <path d="M4 15a8 8 0 0 1 16 0M7.5 15a4.5 4.5 0 0 1 9 0" />
                    <circle cx="12" cy="15" r="1.25" fill="currentColor" stroke="none" />
                  </svg>
                )}
                <span className="absolute inset-0 flex items-center justify-center bg-black/0 transition-colors group-hover:bg-black/25">
                  <svg viewBox="0 0 24 24" className="h-9 w-9 text-white/80 opacity-0 drop-shadow transition-opacity group-hover:opacity-100">
                    <path d="M8 5v14l11-7z" fill="currentColor" />
                  </svg>
                </span>
              </div>
              <div className="p-2">
                <p className="line-clamp-2 text-xs font-medium text-zinc-900 dark:text-zinc-100">{v.title}</p>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function SourcesTab() {
  const [sources, setSources] = useState<YoutubeSource[]>([]);
  const [sourceSettings, setSourceSettings] = useState<SourceSettings[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  function refresh() {
    setLoading(true);
    Promise.all([api.listYoutubeSources(), api.listSourceSettings()])
      .then(([s, settings]) => {
        setSources(s);
        setSourceSettings(settings);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }

  useEffect(refresh, []);

  async function toggleCuration(id: string, enabled: boolean) {
    setBusyId(id);
    try {
      await api.setSourceCurationEnabled(id, enabled);
      refresh();
    } catch (e) {
      console.error(`[popcorn] no se pudo togglear curación de ${id}: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      {loading && <p className="text-xs text-zinc-500 dark:text-zinc-400">Cargando fuentes…</p>}
      {error && <p className="text-xs text-red-500">{error}</p>}
      {!loading && sources.length === 0 && (
        <p className="text-xs text-zinc-500 dark:text-zinc-400">
          No hay canales agregados — usa el botón "+" de arriba para agregar uno.
        </p>
      )}

      <ul className="flex flex-col gap-2">
        {sources.map((s) => {
          const settings = sourceSettings.find((ss) => ss.id === s.id);
          return (
            <li
              key={s.id}
              className="flex flex-col gap-1.5 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="min-w-0">
                  <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{s.name}</p>
                  <p className="truncate text-[10px] text-zinc-500">
                    {s.channel_url} · {s.category}
                  </p>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  <button
                    onClick={() => api.toggleYoutubeSource(s.id, !s.enabled).then(refresh)}
                    className="rounded-md border border-zinc-300 px-2 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
                  >
                    {s.enabled ? "Desactivar" : "Activar"}
                  </button>
                  <button
                    onClick={() => api.removeYoutubeSource(s.id).then(refresh)}
                    className="rounded-md border border-red-300 px-2 py-1 text-[11px] text-red-600 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40"
                  >
                    Quitar
                  </button>
                </div>
              </div>
              {settings && (
                <label className="flex items-center gap-1.5 text-[11px] text-zinc-600 dark:text-zinc-300">
                  <input
                    type="checkbox"
                    checked={settings.curation_enabled}
                    onChange={(e) => toggleCuration(s.id, e.target.checked)}
                    disabled={busyId === s.id}
                  />
                  Curación por IA
                </label>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
