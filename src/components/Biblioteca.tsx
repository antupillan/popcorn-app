import type { LocalFile, MediaItem } from "../types";
import { MediaLibrary } from "./MediaLibrary";
import { OnlineLibraryTab } from "./OnlineLibraryTab";
import { LocalLibraryTab } from "./LocalLibraryTab";
import { IptvView } from "./IptvView";
import { YoutubeView } from "./YoutubeView";

export type BibliotecaTab = "online" | "local" | "iptv" | "youtube" | "coleccion";
type Tab = BibliotecaTab;

const TABS: { id: Tab; label: string }[] = [
  { id: "online", label: "Torrents" },
  { id: "local", label: "Local" },
  { id: "iptv", label: "IPTV" },
  { id: "youtube", label: "YouTube" },
  { id: "coleccion", label: "Mi Colección" },
];

interface BibliotecaProps {
  activeTab: BibliotecaTab;
  onTabChange: (tab: BibliotecaTab) => void;
  mediaItems: MediaItem[];
  onPlayMedia: (item: MediaItem) => void;
  onPlayChannel: (channel: { title: string; url: string; sourceId: string | null }) => void;
  onPlayLocal: (file: LocalFile) => void;
  onPlayOnline: (title: string, url: string) => void;
  onPlayRecording: (recording: { id: string; name: string; durationSeconds: number }) => void;
  onPlayYoutube: (video: { videoId: string; title: string }) => void;
  onMediaAdded: () => void;
  onMediaRemoved: () => void;
}

// Tabs de nivel superior con tipografía más pesada que los sub-tabs de
// IptvView (Canales/Fuentes/Grabaciones) — la jerarquía anidada (IPTV
// adentro de Biblioteca) tiene que leerse a simple vista, no como una
// lista plana de pestañas iguales.
export function Biblioteca({
  activeTab: tab,
  onTabChange,
  mediaItems,
  onPlayMedia,
  onPlayChannel,
  onPlayLocal,
  onPlayOnline,
  onPlayRecording,
  onPlayYoutube,
  onMediaAdded,
  onMediaRemoved,
}: BibliotecaProps) {
  return (
    <div>
      {/* z-20: el badge "En tu colección" de OnlineLibraryTab usa z-10 sobre
          cada card — con el mismo valor, empataban y el badge (más profundo
          en el árbol) ganaba el desempate por orden de DOM, apareciendo
          sobre esta cinta sticky al scrollear el grid (bug reportado en vivo). */}
      <div className="sticky top-0 z-20 flex gap-1 border-b border-zinc-200 bg-slate-50 px-4 pt-3 dark:border-zinc-800 dark:bg-zinc-950">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => onTabChange(t.id)}
            className={`rounded-t-md px-3.5 py-2 text-sm font-semibold transition-colors ${
              tab === t.id
                ? "border-b-2 border-[var(--accent)] text-[var(--accent)] dark:text-[var(--accent-fg)]"
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
      {tab === "iptv" && <IptvView onPlayChannel={onPlayChannel} onPlayRecording={onPlayRecording} />}
      {tab === "youtube" && <YoutubeView onPlayVideo={onPlayYoutube} />}
      {tab === "coleccion" && (
        <MediaLibrary items={mediaItems} onPlay={onPlayMedia} onRemoved={onMediaRemoved} />
      )}
    </div>
  );
}
