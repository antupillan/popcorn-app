import { useCallback, useEffect, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import type { View } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { AddTorrentModal } from "./components/AddTorrentModal";
import { Biblioteca } from "./components/Biblioteca";
import { VideoPlayer } from "./components/VideoPlayer";
import { FirstRunScreen } from "./components/FirstRunScreen";
import { SeedRatioDialog } from "./components/SeedRatioDialog";
import { Ajustes } from "./components/Ajustes";
import { TitleBar } from "./components/TitleBar";
import { api } from "./lib/api";
import type { MediaItem, TorrentInfo } from "./types";

export type Theme = "light" | "dark" | "system";
export type TitleBarSide = "left" | "right";
export type TitleBarOrder = "closeFirst" | "minimizeFirst";

type Playing =
  | { kind: "media"; item: MediaItem }
  | { kind: "channel"; title: string; url: string; sourceId: string | null }
  | { kind: "local"; path: string; name: string }
  | { kind: "online"; title: string; url: string }
  | { kind: "recording"; id: string; name: string; durationSeconds: number };

const FIRST_RUN_KEY = "popcorn.acceptedFirstRun";
const THEME_KEY = "popcorn.theme";
const TITLEBAR_SIDE_KEY = "popcorn.titleBarControlsSide";
const TITLEBAR_ORDER_KEY = "popcorn.titleBarButtonOrder";

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
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem(THEME_KEY);
    return saved === "light" || saved === "dark" || saved === "system" ? saved : "system";
  });
  const [systemPrefersDark, setSystemPrefersDark] = useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const [titleBarSide, setTitleBarSide] = useState<TitleBarSide>(
    () => (localStorage.getItem(TITLEBAR_SIDE_KEY) as TitleBarSide | null) ?? "right",
  );
  const [titleBarOrder, setTitleBarOrder] = useState<TitleBarOrder>(
    () => (localStorage.getItem(TITLEBAR_ORDER_KEY) as TitleBarOrder | null) ?? "minimizeFirst",
  );
  const [view, setView] = useState<View>("biblioteca");
  const [modalOpen, setModalOpen] = useState(false);
  const [ajustesOpen, setAjustesOpen] = useState(false);
  const [torrentsOpen, setTorrentsOpen] = useState(false);
  const [playing, setPlaying] = useState<Playing | null>(null);
  const [seedRatioTorrent, setSeedRatioTorrent] = useState<TorrentInfo | null>(null);
  const [engineFallbackWarning, setEngineFallbackWarning] = useState<string | null>(null);

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setSystemPrefersDark(e.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  // Lectura una sola vez al montar — sin escucha de cambios en vivo del
  // portal. Silencioso ante null/error: el fallback --accent del CSS ya es
  // el sky-600 fijo de siempre.
  useEffect(() => {
    api
      .getOsAccentColor()
      .then((hex) => {
        if (hex) document.documentElement.style.setProperty("--accent", hex);
      })
      .catch(() => {});
  }, []);

  // Se pregunta una sola vez al montar — el motor activo ya quedó decidido
  // en el arranque de Rust (lib.rs::setup), esto solo refleja el resultado.
  useEffect(() => {
    api.getEngineFallbackWarning().then(setEngineFallbackWarning).catch(() => {});
  }, []);

  const isDark = theme === "system" ? systemPrefersDark : theme === "dark";

  useEffect(() => {
    document.documentElement.classList.toggle("dark", isDark);
    localStorage.setItem(THEME_KEY, theme);
  }, [isDark, theme]);

  function cycleTheme() {
    setTheme((t) => (t === "system" ? "light" : t === "light" ? "dark" : "system"));
  }

  function setTitleBarSideAndPersist(side: TitleBarSide) {
    localStorage.setItem(TITLEBAR_SIDE_KEY, side);
    setTitleBarSide(side);
  }

  function setTitleBarOrderAndPersist(order: TitleBarOrder) {
    localStorage.setItem(TITLEBAR_ORDER_KEY, order);
    setTitleBarOrder(order);
  }

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

  // Aviso único al llegar a 1:1 en un torrent sembrado automáticamente (ver
  // plan de sembrado automático) — un diálogo a la vez, marca en
  // localStorage para no repetir por ese id.
  useEffect(() => {
    if (seedRatioTorrent) return;
    const candidate = torrents.find(
      (t) =>
        t.total_bytes > 0 &&
        t.uploaded_bytes >= t.total_bytes &&
        localStorage.getItem(`popcorn.seedNotified.${t.id}`) !== "1",
    );
    if (candidate) setSeedRatioTorrent(candidate);
  }, [torrents, seedRatioTorrent]);

  function dismissSeedRatio(keep: boolean) {
    if (!seedRatioTorrent) return;
    localStorage.setItem(`popcorn.seedNotified.${seedRatioTorrent.id}`, "1");
    if (!keep) {
      api.removeTorrent(seedRatioTorrent.id, false).then(refreshTorrents);
    }
    setSeedRatioTorrent(null);
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
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-slate-50 text-slate-900 dark:bg-zinc-950 dark:text-zinc-100">
      <TitleBar controlsSide={titleBarSide} buttonOrder={titleBarOrder} />

      {engineFallbackWarning && (
        <div className="flex items-center justify-between gap-2 bg-amber-500/15 px-3 py-1.5 text-[11px] text-amber-700 dark:text-amber-400">
          <span>{engineFallbackWarning}</span>
          <button
            onClick={() => setEngineFallbackWarning(null)}
            className="shrink-0 rounded px-1.5 py-0.5 font-semibold hover:bg-amber-500/20"
          >
            Cerrar
          </button>
        </div>
      )}

      <div className="flex flex-1 overflow-hidden">
        <Sidebar active={view} onSelect={setView} onOpenAjustes={() => setAjustesOpen(true)} />

        <div className="flex flex-1 flex-col overflow-hidden">
          <TopBar
            downloadSpeedMbps={totalDown}
            uploadSpeedMbps={totalUp}
            torrents={torrents}
            torrentsOpen={torrentsOpen}
            onToggleTorrents={() => setTorrentsOpen((o) => !o)}
            onTorrentsChanged={refreshTorrents}
            theme={theme}
            onCycleTheme={cycleTheme}
            onOpenAddTorrent={() => setModalOpen(true)}
          />

          <main className="flex-1 overflow-y-auto">
            {view === "biblioteca" && (
              <Biblioteca
                mediaItems={mediaItems}
                onPlayMedia={(item) => setPlaying({ kind: "media", item })}
                onPlayChannel={(channel) => setPlaying({ kind: "channel", ...channel })}
                onPlayLocal={(file) => setPlaying({ kind: "local", path: file.path, name: file.name })}
                onPlayOnline={(title, url) => setPlaying({ kind: "online", title, url })}
                onPlayRecording={(recording) => setPlaying({ kind: "recording", ...recording })}
                onMediaAdded={refreshMedia}
                onMediaRemoved={refreshMedia}
              />
            )}
          </main>
        </div>
      </div>

      {modalOpen && (
        <AddTorrentModal onClose={() => setModalOpen(false)} onAdded={handleAdded} />
      )}
      {playing?.kind === "media" && (
        <VideoPlayer kind="media" item={playing.item} onClose={() => setPlaying(null)} />
      )}
      {playing?.kind === "channel" && (
        <VideoPlayer
          kind="channel"
          title={playing.title}
          url={playing.url}
          sourceId={playing.sourceId}
          onClose={() => setPlaying(null)}
        />
      )}
      {playing?.kind === "local" && (
        <VideoPlayer kind="local" path={playing.path} name={playing.name} onClose={() => setPlaying(null)} />
      )}
      {playing?.kind === "online" && (
        <VideoPlayer kind="online" title={playing.title} url={playing.url} onClose={() => setPlaying(null)} />
      )}
      {playing?.kind === "recording" && (
        <VideoPlayer
          kind="recording"
          id={playing.id}
          name={playing.name}
          durationSeconds={playing.durationSeconds}
          onClose={() => setPlaying(null)}
        />
      )}
      {seedRatioTorrent && (
        <SeedRatioDialog
          torrentName={seedRatioTorrent.name ?? seedRatioTorrent.info_hash}
          onKeep={() => dismissSeedRatio(true)}
          onRemove={() => dismissSeedRatio(false)}
        />
      )}
      {ajustesOpen && (
        <Ajustes
          onClose={() => setAjustesOpen(false)}
          titleBarSide={titleBarSide}
          onSetTitleBarSide={setTitleBarSideAndPersist}
          titleBarOrder={titleBarOrder}
          onSetTitleBarOrder={setTitleBarOrderAndPersist}
        />
      )}
    </div>
  );
}

export default App;
