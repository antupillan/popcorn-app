import { useState } from "react";
import type { ChangeEvent } from "react";
import { api } from "../lib/api";

interface AddTorrentModalBodyProps {
  onClose: () => void;
  onAdded: () => void;
}

type Tab = "magnet" | "file";

const TABS: { id: Tab; label: string }[] = [
  { id: "magnet", label: "Magnet" },
  { id: "file", label: "Subir .torrent" },
];

// Sin backdrop/header propio a propósito — el chrome del modal (backdrop,
// título, botón de cerrar) vive en AddSourceModal, que monta este cuerpo
// como uno de sus tres grupos (torrent/IPTV/YouTube). "Buscar" (archive.org)
// y "Mis Indexers" vivían acá — sacados a pedido del usuario una vez que la
// lupa global ganó el mismo alcance (búsqueda en vivo de archive.org +
// paridad completa de indexers con salud/tags/dedup, ver
// Planes_mejora_popcorn/PlanesImplementados/busqueda_global.txt y
// detector_salud_indexers.txt) — quedaban duplicados sin aportar nada que
// la lupa no tuviera ya. Este modal queda solo para alta manual directa.
export function AddTorrentModalBody({ onClose, onAdded }: AddTorrentModalBodyProps) {
  const [tab, setTab] = useState<Tab>("magnet");

  return (
    <>
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
        {tab === "magnet" && <MagnetTab onAdded={onAdded} onClose={onClose} />}
        {tab === "file" && <FileTab onAdded={onAdded} onClose={onClose} />}
      </div>
    </>
  );
}

interface TabProps {
  onAdded: () => void;
  onClose: () => void;
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
        className="rounded-lg border border-zinc-300 bg-white px-3 py-2 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
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
      <label className="flex cursor-pointer flex-col items-center gap-2 rounded-lg border-2 border-dashed border-zinc-300 px-4 py-8 text-center text-xs text-zinc-500 hover:border-[var(--accent-hover)] hover:text-[var(--accent)] dark:border-zinc-700 dark:hover:border-[var(--accent-hover)]">
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
