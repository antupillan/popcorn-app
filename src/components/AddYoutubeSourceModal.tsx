import { useState } from "react";
import { api } from "../lib/api";

interface AddYoutubeSourceModalBodyProps {
  onClose: () => void;
  onAdded: () => void;
}

const CATEGORIES: { id: "cine" | "series" | "anime"; label: string }[] = [
  { id: "cine", label: "Cine" },
  { id: "series", label: "Series" },
  { id: "anime", label: "Anime" },
];

// El canal es tuyo — mismo blindaje legal que indexers/IPTV (BYO, sin
// catálogo propio recomendado por la app). Distribuidores regionales como
// Muse Asia/Ani-One tienen region-lock real confirmado (ver
// fuente_youtube.txt) — el aviso va acá, antes de que el usuario configure
// algo esperando que reproduzca fuera de esa región.
// Sin backdrop/header propio: el chrome vive en AddSourceModal, que monta
// este cuerpo como uno de sus tres grupos.
export function AddYoutubeSourceModalBody({ onClose, onAdded }: AddYoutubeSourceModalBodyProps) {
  const [name, setName] = useState("");
  const [channelUrl, setChannelUrl] = useState("");
  const [category, setCategory] = useState<"cine" | "series" | "anime">("anime");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function add() {
    if (!name.trim() || !channelUrl.trim()) return;
    setLoading(true);
    setError(null);
    try {
      await api.addYoutubeSource(name.trim(), channelUrl.trim(), category);
      onAdded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="flex-1 overflow-y-auto">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            add();
          }}
          className="flex flex-col gap-3 p-4"
        >
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            El canal es tuyo — Popcorn no recomienda ninguno. Algunos distribuidores regionales
            (ej. Muse Asia, Ani-One) bloquean su contenido fuera de ciertos países por licencia
            comercial: eso es normal de YouTube, no algo que Popcorn pueda evitar.
          </p>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Nombre de la fuente"
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />
          <input
            value={channelUrl}
            onChange={(e) => setChannelUrl(e.target.value)}
            placeholder="https://www.youtube.com/@canal o /channel/UC..."
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />
          <div className="flex gap-1.5">
            {CATEGORIES.map((c) => (
              <button
                key={c.id}
                type="button"
                onClick={() => setCategory(c.id)}
                className={`flex-1 rounded-lg border px-3 py-1.5 text-xs font-medium ${
                  category === c.id
                    ? "border-[var(--accent)] bg-[var(--accent)] text-white"
                    : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
                }`}
              >
                {c.label}
              </button>
            ))}
          </div>
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
  );
}
