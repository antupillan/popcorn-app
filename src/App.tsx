import { useCallback, useEffect, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import type { View } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { AddTorrentModal } from "./components/AddTorrentModal";
import { TorrentList } from "./components/TorrentList";
import { MediaLibrary } from "./components/MediaLibrary";
import { VideoPlayer } from "./components/VideoPlayer";
import { FirstRunScreen } from "./components/FirstRunScreen";
import { api } from "./lib/api";
import type { MediaItem, TorrentInfo } from "./types";

const FIRST_RUN_KEY = "popcorn.acceptedFirstRun";
const THEME_KEY = "popcorn.theme";

function usePolling<T>(fetcher: () => Promise<T>, intervalMs: number, initial: T) {
  const [data, setData] = useState<T>(initial);

  const refresh = useCallback(() => {
    fetcher().then(setData).catch(console.error);
  }, [fetcher]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, intervalMs);
    return () => clearInterval(id);
  }, [refresh, intervalMs]);

  return { data, refresh };
}

function App() {
  const [acceptedFirstRun, setAcceptedFirstRun] = useState(
    () => localStorage.getItem(FIRST_RUN_KEY) === "1",
  );
  const [isDark, setIsDark] = useState(() => localStorage.getItem(THEME_KEY) !== "light");
  const [view, setView] = useState<View>("library");
  const [modalOpen, setModalOpen] = useState(false);
  const [playing, setPlaying] = useState<MediaItem | null>(null);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", isDark);
    localStorage.setItem(THEME_KEY, isDark ? "dark" : "light");
  }, [isDark]);

  const { data: torrents, refresh: refreshTorrents } = usePolling<TorrentInfo[]>(
    api.listTorrents,
    2000,
    [],
  );
  const { data: mediaItems, refresh: refreshMedia } = usePolling<MediaItem[]>(
    api.listMediaItems,
    5000,
    [],
  );

  function handleAdded() {
    refreshTorrents();
    refreshMedia();
  }

  if (!acceptedFirstRun) {
    return (
      <FirstRunScreen
        onAccept={() => {
          localStorage.setItem(FIRST_RUN_KEY, "1");
          setAcceptedFirstRun(true);
        }}
      />
    );
  }

  const totalDown = torrents.reduce((acc, t) => acc + t.download_speed_mbps, 0);
  const totalUp = torrents.reduce((acc, t) => acc + t.upload_speed_mbps, 0);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-slate-50 text-slate-900 dark:bg-zinc-950 dark:text-zinc-100">
      <Sidebar active={view} onSelect={setView} torrentCount={torrents.length} />

      <div className="flex flex-1 flex-col overflow-hidden">
        <TopBar
          downloadSpeedMbps={totalDown}
          uploadSpeedMbps={totalUp}
          isDark={isDark}
          onToggleTheme={() => setIsDark((d) => !d)}
          onOpenAddTorrent={() => setModalOpen(true)}
        />

        <main className="flex-1 overflow-y-auto">
          {view === "library" && (
            <MediaLibrary items={mediaItems} onPlay={setPlaying} />
          )}
          {view === "torrents" && (
            <TorrentList torrents={torrents} onChanged={refreshTorrents} />
          )}
        </main>
      </div>

      {modalOpen && (
        <AddTorrentModal onClose={() => setModalOpen(false)} onAdded={handleAdded} />
      )}
      {playing && <VideoPlayer item={playing} onClose={() => setPlaying(null)} />}
    </div>
  );
}

export default App;
