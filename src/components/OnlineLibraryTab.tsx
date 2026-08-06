import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { OnlineItem } from "../types";

const KIND_LABEL: Record<OnlineItem["kind"], string> = {
  archive_org: "archive.org",
  public_domain_torrents: "Public Domain Torrents",
  blender_foundation: "Blender Foundation",
};

interface OnlineLibraryTabProps {
  onAdded: () => void;
}

export function OnlineLibraryTab({ onAdded }: OnlineLibraryTabProps) {
  const [items, setItems] = useState<OnlineItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setLoading(true);
    api
      .browseOnlineLibrary()
      .then(setItems)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  async function add(item: OnlineItem) {
    const key = `${item.kind}:${item.identifier}`;
    setBusyKey(key);
    setMessage(null);
    try {
      await api.addOnlineItem(item.kind, item.identifier, item.title, item.year, item.license);
      setMessage(`Agregado: ${item.title} (ver pestaña Mi Colección).`);
      onAdded();
    } catch (e) {
      setMessage(`No se pudo agregar "${item.title}": ${e}`);
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
  if (items.length === 0) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
        No hay resultados en el catálogo Online por ahora.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-2 p-4">
      {message && <p className="text-xs text-sky-600 dark:text-sky-400">{message}</p>}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        {items.map((item) => {
          const key = `${item.kind}:${item.identifier}`;
          return (
            <div
              key={key}
              className="flex flex-col overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-zinc-800 dark:bg-zinc-900"
            >
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
                <button
                  onClick={() => add(item)}
                  disabled={busyKey === key}
                  className="mt-1 rounded-md border border-sky-600 px-2 py-1 text-[11px] font-semibold text-sky-600 hover:bg-sky-600 hover:text-white disabled:opacity-50 dark:text-sky-400"
                >
                  {busyKey === key ? "Agregando…" : "Agregar"}
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
