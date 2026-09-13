import { useState } from "react";
import { X } from "lucide-react";
import { AddTorrentModalBody } from "./AddTorrentModal";
import { AddIptvSourceModalBody } from "./AddIptvSourceModal";
import { AddYoutubeSourceModalBody } from "./AddYoutubeSourceModal";

export type SourceGroup = "torrent" | "iptv" | "youtube";

const GROUPS: { id: SourceGroup; label: string; title: string }[] = [
  { id: "torrent", label: "Torrent", title: "Agregar torrent" },
  { id: "iptv", label: "IPTV", title: "Agregar fuente IPTV" },
  { id: "youtube", label: "YouTube", title: "Agregar canal de YouTube" },
];

interface AddSourceModalProps {
  initialGroup: SourceGroup;
  onClose: () => void;
  onTorrentAdded: () => void;
  onIptvAdded?: () => void;
  onYoutubeAdded?: () => void;
}

// Punto de entrada único para agregar cualquier tipo de fuente — antes
// eran 3 botones separados (TopBar, y uno contextual dentro de cada tab
// de Biblioteca), cada uno una función parcial. El chrome (backdrop,
// header, cierre) vive acá; cada grupo delega en el cuerpo del modal que
// ya existía para ese tipo (AddTorrentModalBody/AddIptvSourceModalBody/
// AddYoutubeSourceModalBody), sin tocar su lógica interna.
export function AddSourceModal({
  initialGroup,
  onClose,
  onTorrentAdded,
  onIptvAdded,
  onYoutubeAdded,
}: AddSourceModalProps) {
  const [group, setGroup] = useState<SourceGroup>(initialGroup);
  const active = GROUPS.find((g) => g.id === group)!;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-zinc-200/50 bg-white/80 shadow-xl backdrop-blur-xl dark:border-zinc-800/50 dark:bg-zinc-900/80">
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">{active.title}</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Tira de primer nivel, tipografía más pesada que las sub-tabs
            internas de cada cuerpo — misma jerarquía visual que ya usa
            Biblioteca.tsx entre sus tabs y las sub-tabs de IptvView. */}
        <div className="flex gap-1 border-b border-zinc-200 bg-slate-50 px-3 pt-2 dark:border-zinc-800 dark:bg-zinc-950">
          {GROUPS.map((g) => (
            <button
              key={g.id}
              onClick={() => setGroup(g.id)}
              className={`rounded-t-md px-3 py-1.5 text-sm font-semibold transition-colors ${
                group === g.id
                  ? "border-b-2 border-[var(--accent)] text-[var(--accent)] dark:text-[var(--accent-fg)]"
                  : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
              }`}
            >
              {g.label}
            </button>
          ))}
        </div>

        {group === "torrent" && <AddTorrentModalBody onClose={onClose} onAdded={onTorrentAdded} />}
        {group === "iptv" && <AddIptvSourceModalBody onClose={onClose} onAdded={onIptvAdded ?? (() => {})} />}
        {group === "youtube" && <AddYoutubeSourceModalBody onClose={onClose} onAdded={onYoutubeAdded ?? (() => {})} />}
      </div>
    </div>
  );
}
