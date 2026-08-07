import { useState } from "react";
import type { LocalFile, MediaItem } from "../types";
import { MediaLibrary } from "./MediaLibrary";
import { OnlineLibraryTab } from "./OnlineLibraryTab";
import { LocalLibraryTab } from "./LocalLibraryTab";
import { IptvView } from "./IptvView";

type Tab = "online" | "local" | "iptv" | "coleccion";

const TABS: { id: Tab; label: string }[] = [
  { id: "online", label: "Online" },
  { id: "local", label: "Local" },
  { id: "iptv", label: "IPTV" },
  { id: "coleccion", label: "Mi Colección" },
];

interface BibliotecaProps {
  mediaItems: MediaItem[];
  onPlayMedia: (item: MediaItem) => void;
  onPlayChannel: (channel: { title: string; url: string }) => void;
  onPlayLocal: (file: LocalFile) => void;
  onPlayOnline: (title: string, url: string) => void;
  onMediaAdded: () => void;
}

// Tabs de nivel superior con tipografía más pesada que los sub-tabs de
// IptvView (Canales/Fuentes/Grabaciones) — la jerarquía anidada (IPTV
// adentro de Biblioteca) tiene que leerse a simple vista, no como una
// lista plana de pestañas iguales.
export function Biblioteca({
  mediaItems,
  onPlayMedia,
  onPlayChannel,
  onPlayLocal,
  onPlayOnline,
  onMediaAdded,
}: BibliotecaProps) {
  const [tab, setTab] = useState<Tab>("online");

  return (
    <div>
      <div className="sticky top-0 z-10 flex gap-1 border-b border-zinc-200 bg-slate-50 px-4 pt-3 dark:border-zinc-800 dark:bg-zinc-950">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`rounded-t-md px-3.5 py-2 text-sm font-semibold transition-colors ${
              tab === t.id
                ? "border-b-2 border-sky-600 text-sky-600 dark:text-sky-400"
                : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "online" && (
        <OnlineLibraryTab
          mediaItems={mediaItems}
          onPlayMedia={onPlayMedia}
          onPlayOnline={onPlayOnline}
          onAdded={onMediaAdded}
        />
      )}
      {tab === "local" && <LocalLibraryTab onPlayLocal={onPlayLocal} />}
      {tab === "iptv" && <IptvView onPlayChannel={onPlayChannel} />}
      {tab === "coleccion" && <MediaLibrary items={mediaItems} onPlay={onPlayMedia} />}
    </div>
  );
}
