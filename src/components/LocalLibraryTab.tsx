import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import type { LocalFile } from "../types";

interface LocalLibraryTabProps {
  onPlayLocal: (file: LocalFile) => void;
}

export function LocalLibraryTab({ onPlayLocal }: LocalLibraryTabProps) {
  const [folder, setFolder] = useState<string | null>(null);
  const [files, setFiles] = useState<LocalFile[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  function refreshFiles() {
    api
      .listLocalFiles()
      .then(setFiles)
      .catch((e) => setError(String(e)));
  }

  useEffect(() => {
    setLoading(true);
    api
      .getLocalLibraryFolder()
      .then((f) => {
        setFolder(f);
        if (f) refreshFiles();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  async function pickFolder() {
    const selected = await open({ directory: true });
    if (!selected || Array.isArray(selected)) return;
    setError(null);
    await api.setLocalLibraryFolder(selected);
    setFolder(selected);
    refreshFiles();
  }

  if (loading) {
    return (
      <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">Cargando biblioteca local…</p>
    );
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex items-center justify-between gap-2">
        <p className="min-w-0 truncate text-xs text-zinc-600 dark:text-zinc-400">
          {folder ?? "Sin carpeta configurada"}
        </p>
        <button
          onClick={pickFolder}
          className="shrink-0 rounded-lg bg-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-white hover:bg-[var(--accent-hover)]"
        >
          {folder ? "Cambiar carpeta" : "Elegir carpeta"}
        </button>
      </div>

      {error && <p className="text-xs text-red-500">{error}</p>}

      {!folder && (
        <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
          Elige una carpeta para ver tus archivos de video locales aquí.
        </p>
      )}

      {folder && files.length === 0 && !error && (
        <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
          No se encontraron archivos de video en esa carpeta.
        </p>
      )}

      {files.length > 0 && (
        <ul className="flex flex-col gap-1.5">
          {files.map((f) => (
            <li
              key={f.path}
              className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 dark:border-zinc-800"
            >
              <p className="min-w-0 truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{f.name}</p>
              <button
                onClick={() => onPlayLocal(f)}
                className="shrink-0 rounded-md border border-[var(--accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white dark:text-[var(--accent-fg)]"
              >
                Reproducir
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
