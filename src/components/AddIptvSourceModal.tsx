import { useState } from "react";
import type { ChangeEvent } from "react";
import { api } from "../lib/api";

interface AddIptvSourceModalProps {
  onClose: () => void;
  onAdded: () => void;
}

type Tab = "url" | "file";

const TABS: { id: Tab; label: string }[] = [
  { id: "url", label: "URL de lista" },
  { id: "file", label: "Subir .m3u" },
];

// Sin tab de "búsqueda" a propósito — sin catálogo propio de listas IPTV,
// mismo blindaje legal que los indexers BYO: el usuario trae sus propias
// fuentes, la app no recomienda ninguna más allá de la semilla pública ya
// sembrada (ver migración iptv_sources).
export function AddIptvSourceModal({ onClose, onAdded }: AddIptvSourceModalProps) {
  const [tab, setTab] = useState<Tab>("url");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-zinc-200 bg-white shadow-xl dark:border-zinc-800 dark:bg-zinc-900">
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">Agregar fuente IPTV</h2>
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
                  ? "border-b-2 border-[var(--accent)] text-[var(--accent)] dark:text-[var(--accent-fg)]"
                  : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto p-4">
          {tab === "url" && <UrlTab onAdded={onAdded} onClose={onClose} />}
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

function UrlTab({ onAdded, onClose }: TabProps) {
  const [name, setName] = useState("");
  const [playlistUrl, setPlaylistUrl] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function add() {
    if (!name.trim() || !playlistUrl.trim()) return;
    setLoading(true);
    setError(null);
    try {
      await api.addIptvSourceUrl(name.trim(), playlistUrl.trim());
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
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        La lista M3U/M3U8 es tuya — Popcorn no recomienda ni verifica su procedencia, solo la fuente pública ya incluida por defecto.
      </p>
      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="Nombre de la fuente"
        className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
      />
      <input
        value={playlistUrl}
        onChange={(e) => setPlaylistUrl(e.target.value)}
        placeholder="https://.../lista.m3u8"
        className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
      />
      {error && <p className="text-xs text-red-500">{error}</p>}
      <button
        type="submit"
        disabled={loading}
        className="self-end rounded-lg bg-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-white hover:bg-[var(--accent-hover)] disabled:opacity-50"
      >
        {loading ? "Agregando…" : "Agregar"}
      </button>
    </form>
  );
}

function FileTab({ onAdded, onClose }: TabProps) {
  const [name, setName] = useState("");
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
      await api.addIptvSourceFile(name.trim() || file.name, bytes);
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
      <input
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="Nombre de la fuente (opcional, usa el del archivo si se deja vacío)"
        className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
      />
      <label className="flex cursor-pointer flex-col items-center gap-2 rounded-lg border-2 border-dashed border-zinc-300 px-4 py-8 text-center text-xs text-zinc-500 hover:border-[var(--accent-hover)] hover:text-[var(--accent)] dark:border-zinc-700 dark:hover:border-[var(--accent-hover)]">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" className="h-6 w-6">
          <path d="M12 16V4m0 0L7 9m5-5 5 5M5 20h14" />
        </svg>
        {loading ? "Agregando…" : "Click para elegir un archivo .m3u/.m3u8"}
        <input type="file" accept=".m3u,.m3u8" className="hidden" onChange={handleFile} disabled={loading} />
      </label>
      {error && <p className="text-xs text-red-500">{error}</p>}
    </div>
  );
}
