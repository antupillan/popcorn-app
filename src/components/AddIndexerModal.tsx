import { useState } from "react";
import { api } from "../lib/api";
import type { JsonPaths } from "../types";

interface AddIndexerModalProps {
  onClose: () => void;
  onAdded: () => void;
}

type ResultFormat = "magnet_list" | "rss" | "json";

// Sin catálogo propio de indexers a propósito (blindaje legal, ver
// Mandato 5) — el usuario trae su propia plantilla de búsqueda.
export function AddIndexerModal({ onClose, onAdded }: AddIndexerModalProps) {
  const [name, setName] = useState("");
  const [searchUrlTemplate, setSearchUrlTemplate] = useState("");
  const [resultFormat, setResultFormat] = useState<ResultFormat>("magnet_list");
  const [itemsPath, setItemsPath] = useState("");
  const [titleField, setTitleField] = useState("");
  const [magnetField, setMagnetField] = useState("");
  const [sizeField, setSizeField] = useState("");
  const [seedersField, setSeedersField] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function add() {
    if (!name.trim() || !searchUrlTemplate.trim()) return;
    if (resultFormat === "json" && (!titleField.trim() || !magnetField.trim())) return;
    setLoading(true);
    setError(null);
    try {
      const jsonPaths: JsonPaths | null =
        resultFormat === "json"
          ? {
              items_path: itemsPath.trim(),
              title_field: titleField.trim(),
              magnet_field: magnetField.trim(),
              size_field: sizeField.trim() || null,
              seeders_field: seedersField.trim() || null,
            }
          : null;
      await api.addIndexer(name.trim(), searchUrlTemplate.trim(), resultFormat, jsonPaths);
      onAdded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-zinc-200/50 bg-white/80 shadow-xl backdrop-blur-xl dark:border-zinc-800/50 dark:bg-zinc-900/80">
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">Agregar indexer</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            add();
          }}
          className="flex flex-1 flex-col gap-3 overflow-y-auto p-4"
        >
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            El indexer es tuyo — Popcorn no recomienda ni verifica su procedencia.
          </p>

          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Nombre del indexer"
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />
          <input
            value={searchUrlTemplate}
            onChange={(e) => setSearchUrlTemplate(e.target.value)}
            placeholder="URL de búsqueda con {query} (ej. https://.../search?q={query})"
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />
          <select
            value={resultFormat}
            onChange={(e) => setResultFormat(e.target.value as ResultFormat)}
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          >
            <option value="magnet_list">Lista de magnets</option>
            <option value="rss">RSS</option>
            <option value="json">JSON</option>
          </select>

          {resultFormat === "json" && (
            <div className="flex flex-col gap-2 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
              <p className="text-[11px] text-zinc-500 dark:text-zinc-400">
                Dot-path simple sobre la respuesta JSON — vacío en "ruta de items" si la respuesta ya es el array.
              </p>
              <input
                value={itemsPath}
                onChange={(e) => setItemsPath(e.target.value)}
                placeholder="Ruta de items (ej. results.items)"
                className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
              />
              <input
                value={titleField}
                onChange={(e) => setTitleField(e.target.value)}
                placeholder="Campo de título"
                className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
              />
              <input
                value={magnetField}
                onChange={(e) => setMagnetField(e.target.value)}
                placeholder="Campo de magnet"
                className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
              />
              <input
                value={sizeField}
                onChange={(e) => setSizeField(e.target.value)}
                placeholder="Campo de tamaño (opcional)"
                className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
              />
              <input
                value={seedersField}
                onChange={(e) => setSeedersField(e.target.value)}
                placeholder="Campo de seeders (opcional)"
                className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
              />
            </div>
          )}

          {error && <p className="text-xs text-red-500">{error}</p>}

          <button
            type="submit"
            disabled={loading}
            className="self-end rounded-lg bg-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-white hover:bg-[var(--accent-hover)] disabled:opacity-50"
          >
            {loading ? "Agregando…" : "Agregar"}
          </button>
        </form>
      </div>
    </div>
  );
}
