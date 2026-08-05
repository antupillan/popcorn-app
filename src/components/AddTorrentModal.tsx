import { useState } from "react";
import type { ChangeEvent } from "react";
import { api } from "../lib/api";
import type { ArchiveOrgItem } from "../types";

interface AddTorrentModalProps {
  onClose: () => void;
  onAdded: () => void;
}

type Tab = "search" | "magnet" | "file";

const TABS: { id: Tab; label: string }[] = [
  { id: "search", label: "Buscar" },
  { id: "magnet", label: "Magnet" },
  { id: "file", label: "Subir .torrent" },
];

export function AddTorrentModal({ onClose, onAdded }: AddTorrentModalProps) {
  const [tab, setTab] = useState<Tab>("search");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-zinc-200 bg-white shadow-xl dark:border-zinc-800 dark:bg-zinc-900">
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">Agregar Torrent</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="flex gap-1 border-b border-zinc-200 px-3 pt-2 dark:border-zinc-800">
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

        <div className="flex-1 overflow-y-auto p-4">
          {tab === "search" && <SearchTab onAdded={onAdded} onClose={onClose} />}
          {tab === "magnet" && <MagnetTab onAdded={onAdded} onClose={onClose} />}
          {tab === "file" && <FileTab onAdded={onAdded} onClose={onClose} />}
        </div>
      </div>
    </div>
  );
}

interface TabProps {
  onAdded: () => void;
  onClose: () => void;
}

function SearchTab({ onAdded, onClose }: TabProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ArchiveOrgItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [addingId, setAddingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function search() {
    if (!query.trim()) return;
    setLoading(true);
    setError(null);
    try {
      setResults(await api.searchArchiveOrg(query));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function add(item: ArchiveOrgItem) {
    setAddingId(item.identifier);
    setError(null);
    try {
      await api.addArchiveOrgItem(item);
      onAdded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setAddingId(null);
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        Catálogo legal (archive.org — dominio público y licencia abierta).
      </p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          search();
        }}
        className="flex gap-2"
      >
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Título a buscar..."
          className="flex-1 rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-sky-500 dark:border-zinc-700 dark:bg-zinc-950"
        />
        <button
          type="submit"
          disabled={loading}
          className="rounded-lg bg-sky-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-sky-500 disabled:opacity-50"
        >
          {loading ? "Buscando…" : "Buscar"}
        </button>
      </form>

      {error && <p className="text-xs text-red-500">{error}</p>}

      <ul className="flex flex-col gap-1.5">
        {results.map((item) => (
          <li
            key={item.identifier}
            className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 dark:border-zinc-800"
          >
            <div className="flex min-w-0 items-center gap-2.5">
              <img
                src={item.thumbnail_url}
                alt=""
                className="h-10 w-10 shrink-0 rounded-md object-cover"
                loading="lazy"
                onError={(e) => (e.currentTarget.style.visibility = "hidden")}
              />
              <div className="min-w-0">
                <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">
                  {item.title}
                </p>
                <p className="text-[10px] text-zinc-500">{item.year ?? "—"}</p>
              </div>
            </div>
            <button
              onClick={() => add(item)}
              disabled={addingId === item.identifier}
              className="shrink-0 rounded-md border border-sky-600 px-2.5 py-1 text-[11px] font-semibold text-sky-600 hover:bg-sky-600 hover:text-white disabled:opacity-50 dark:text-sky-400"
            >
              {addingId === item.identifier ? "Agregando…" : "Agregar"}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function MagnetTab({ onAdded, onClose }: TabProps) {
  const [magnet, setMagnet] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function add() {
    if (!magnet.trim()) return;
    setLoading(true);
    setError(null);
    try {
      await api.addTorrent(magnet.trim());
      onAdded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        add();
      }}
      className="flex flex-col gap-3"
    >
      <textarea
        value={magnet}
        onChange={(e) => setMagnet(e.target.value)}
        placeholder="magnet:?xt=urn:btih:..."
        rows={4}
        className="rounded-lg border border-zinc-300 bg-white px-3 py-2 text-xs outline-none focus:border-sky-500 dark:border-zinc-700 dark:bg-zinc-950"
      />
      {error && <p className="text-xs text-red-500">{error}</p>}
      <button
        type="submit"
        disabled={loading}
        className="self-end rounded-lg bg-sky-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-sky-500 disabled:opacity-50"
      >
        {loading ? "Agregando…" : "Agregar"}
      </button>
    </form>
  );
}

function FileTab({ onAdded, onClose }: TabProps) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setLoading(true);
    setError(null);
    try {
      const buf = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      await api.addTorrentFile(bytes);
      onAdded();
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <label className="flex cursor-pointer flex-col items-center gap-2 rounded-lg border-2 border-dashed border-zinc-300 px-4 py-8 text-center text-xs text-zinc-500 hover:border-sky-500 hover:text-sky-600 dark:border-zinc-700 dark:hover:border-sky-500">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" className="h-6 w-6">
          <path d="M12 16V4m0 0L7 9m5-5 5 5M5 20h14" />
        </svg>
        {loading ? "Agregando…" : "Click para elegir un archivo .torrent"}
        <input type="file" accept=".torrent" className="hidden" onChange={handleFile} disabled={loading} />
      </label>
      {error && <p className="text-xs text-red-500">{error}</p>}
    </div>
  );
}
