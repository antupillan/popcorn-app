import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { parseTorrentTags } from "../lib/torrentTags";
import type { ArchiveOrgItem, Channel, IndexerResult, MediaItem, OnlineItem, TorrentHealth, YoutubeVideo } from "../types";

type ResultKind = "media" | "channel" | "youtube" | "online";

interface CombinedItem {
  kind: ResultKind;
  key: string;
  title: string;
  media?: MediaItem;
  channel?: Channel;
  video?: YoutubeVideo;
  online?: OnlineItem;
}

interface SearchModalProps {
  onClose: () => void;
  onPlayMedia: (item: MediaItem) => void;
  onPlayChannel: (channel: { title: string; url: string; sourceId: string | null }) => void;
  onPlayYoutube: (video: { videoId: string; title: string }) => void;
  onPlayOnline: (title: string, url: string) => void;
  onMediaAdded: () => void;
}

type Tab = "todo" | "torrents" | "indexers" | "iptv" | "youtube";

const TABS: { id: Tab; label: string }[] = [
  { id: "todo", label: "Todo" },
  { id: "torrents", label: "Torrents" },
  { id: "indexers", label: "Mis Indexers" },
  { id: "iptv", label: "IPTV" },
  { id: "youtube", label: "YouTube" },
];

const KIND_LABEL: Record<ResultKind, string> = { media: "Mi Colección", channel: "IPTV", youtube: "YouTube", online: "Online" };

// Mismo key que AddTorrentModal.tsx — es una preferencia del usuario, no
// algo propio de cada modal, así que se comparte entre "+" y la lupa.
const AI_SEARCH_KEY = "popcorn.indexers.aiSearch";

// Lupa global (TopBar). Antes mezclaba todo en una lista plana con un
// selector ⚙ que hacía doble función (qué entra en el filtro instantáneo
// Y qué se busca en profundidad) — confuso, no quedaba claro qué se
// estaba buscando (reportado en vivo). Rediseñado con pestañas, mismo
// estilo que AddSourceModal: "Todo" es el filtro instantáneo agrupado de
// siempre sobre lo ya agregado; cada pestaña de categoría (Torrents/Mis
// Indexers/IPTV/YouTube) muestra solo lo suyo y expone su propia acción
// de búsqueda en profundidad cuando aplica — ver
// Planes_mejora_popcorn/busqueda_global.txt para el porqué de cada
// límite (nunca search.list de YouTube contra canales no agregados).
export function SearchModal({ onClose, onPlayMedia, onPlayChannel, onPlayYoutube, onPlayOnline, onMediaAdded }: SearchModalProps) {
  const [tab, setTab] = useState<Tab>("todo");
  const [query, setQuery] = useState("");
  const [mediaItems, setMediaItems] = useState<MediaItem[]>([]);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [videos, setVideos] = useState<YoutubeVideo[]>([]);
  const [onlineItems, setOnlineItems] = useState<OnlineItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [playingChannelUrl, setPlayingChannelUrl] = useState<string | null>(null);
  const [onlineBusyKey, setOnlineBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [aiLoading, setAiLoading] = useState(false);
  const [aiLocalResults, setAiLocalResults] = useState<CombinedItem[] | null>(null);

  const [archiveOrgResults, setArchiveOrgResults] = useState<ArchiveOrgItem[] | null>(null);
  const [searchingArchiveOrg, setSearchingArchiveOrg] = useState(false);
  const [addingArchiveOrgId, setAddingArchiveOrgId] = useState<string | null>(null);

  const [indexerResults, setIndexerResults] = useState<IndexerResult[] | null>(null);
  const [searchingIndexers, setSearchingIndexers] = useState(false);
  const [addingMagnet, setAddingMagnet] = useState<string | null>(null);
  const [indexerHealth, setIndexerHealth] = useState<Record<string, TorrentHealth>>({});
  const [checkingHealthMagnet, setCheckingHealthMagnet] = useState<string | null>(null);
  const [aiSearch, setAiSearch] = useState(() => localStorage.getItem(AI_SEARCH_KEY) === "1");

  function toggleAiSearch(value: boolean) {
    setAiSearch(value);
    localStorage.setItem(AI_SEARCH_KEY, value ? "1" : "0");
  }

  const [youtubeDeepResults, setYoutubeDeepResults] = useState<YoutubeVideo[] | null>(null);
  const [searchingYoutubeDeep, setSearchingYoutubeDeep] = useState(false);

  useEffect(() => {
    let ignore = false;
    setLoading(true);
    Promise.all([api.listMediaItems(), api.listChannels(), api.listYoutubeVideos()])
      .then(([m, c, v]) => {
        if (ignore) return;
        setMediaItems(m);
        setChannels(c);
        setVideos(v);
      })
      .catch((e) => !ignore && setError(String(e)))
      .finally(() => !ignore && setLoading(false));

    // Catálogo Online (archive.org, Public Domain Torrents, etc.) — antes
    // vivía como filtro embebido en OnlineLibraryTab, trasladado acá para
    // que la lupa global también lo cubra. Sin curación IA: acá solo hace
    // falta el título para el filtro de texto, no el ranking del grid.
    api
      .browseOnlineLibrary()
      .then((items) => !ignore && setOnlineItems(items))
      .catch(() => {});
    api
      .browsePublicDomainTorrents()
      .then((items) => !ignore && setOnlineItems((prev) => [...prev, ...items]))
      .catch(() => {});

    return () => {
      ignore = true;
    };
  }, []);

  // "Todo": agrupa absolutamente todo lo ya agregado, sin gating por
  // pestaña — las pestañas de categoría de abajo son la forma de acotar,
  // no un toggle aparte.
  const items = useMemo<CombinedItem[]>(() => {
    const out: CombinedItem[] = mediaItems.map((m) => ({ kind: "media", key: `media:${m.id}`, title: m.title, media: m }));
    channels.forEach((c) =>
      out.push({ kind: "channel", key: `channel:${c.source_id}:${c.url}`, title: c.name, channel: c }),
    );
    videos.forEach((v) =>
      out.push({ kind: "youtube", key: `youtube:${v.source_id}:${v.video_id}`, title: v.title, video: v }),
    );
    onlineItems.forEach((o) =>
      out.push({ kind: "online", key: `online:${o.kind}:${o.identifier}`, title: o.title, online: o }),
    );
    return out;
  }, [mediaItems, channels, videos, onlineItems]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((i) => i.title.toLowerCase().includes(q));
  }, [items, query]);

  const filteredOnline = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return onlineItems;
    return onlineItems.filter((o) => o.title.toLowerCase().includes(q));
  }, [onlineItems, query]);

  const filteredChannels = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return channels;
    return channels.filter((c) => c.name.toLowerCase().includes(q));
  }, [channels, query]);

  const filteredVideos = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return videos;
    return videos.filter((v) => v.title.toLowerCase().includes(q));
  }, [videos, query]);

  // "Buscar con IA" en "Todo" solo rankea lo ya agregado (semántico, no
  // texto exacto) — la búsqueda en profundidad contra proveedores externos
  // ahora vive en la acción propia de cada pestaña de categoría.
  async function searchWithAi() {
    if (!query.trim()) return;
    setAiLoading(true);
    setError(null);
    setAiLocalResults(null);
    try {
      const candidates = items.map((i) => i.title);
      const scored = await api.searchAddedContentWithAi(query, candidates);
      const ranked = scored
        .filter((s) => s.score > 0)
        .sort((a, b) => b.score - a.score)
        .map((s) => items[s.index])
        .filter((i): i is CombinedItem => !!i);
      setAiLocalResults(ranked);
    } catch (e) {
      setError(String(e));
    } finally {
      setAiLoading(false);
    }
  }

  // Mismo alcance que "+/Buscar" (archive.org en vivo, no solo el
  // catálogo curado) — unificado acá para poder sacar esa pestaña de "+"
  // sin perder función (decisión explícita del usuario).
  async function searchArchiveOrgLive() {
    if (!query.trim()) return;
    setSearchingArchiveOrg(true);
    setError(null);
    try {
      setArchiveOrgResults(await api.searchArchiveOrg(query));
    } catch (e) {
      setError(String(e));
    } finally {
      setSearchingArchiveOrg(false);
    }
  }

  async function addArchiveOrgResult(item: ArchiveOrgItem) {
    const existing = mediaItems.find((m) => m.source_identifier === item.identifier);
    if (existing) {
      onPlayMedia(existing);
      onClose();
      return;
    }
    setAddingArchiveOrgId(item.identifier);
    setError(null);
    try {
      await api.addArchiveOrgItem(item);
      onMediaAdded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setAddingArchiveOrgId(null);
    }
  }

  async function searchMyIndexers() {
    if (!query.trim()) return;
    setSearchingIndexers(true);
    setError(null);
    try {
      setIndexerResults(aiSearch ? await api.searchIndexersWithAi(query) : await api.searchIndexers(query));
    } catch (e) {
      setError(String(e));
    } finally {
      setSearchingIndexers(false);
    }
  }

  async function searchYoutubeDeep() {
    if (!query.trim()) return;
    setSearchingYoutubeDeep(true);
    setError(null);
    try {
      setYoutubeDeepResults(await api.searchYoutubeVideosInAddedChannels(query));
    } catch (e) {
      setError(String(e));
    } finally {
      setSearchingYoutubeDeep(false);
    }
  }

  async function playChannel(channel: Channel) {
    setPlayingChannelUrl(channel.url);
    setError(null);
    try {
      const validUrl = await api.validateChannelManifest(channel.url);
      onPlayChannel({ title: channel.name, url: validUrl, sourceId: channel.source_id || null });
      onClose();
    } catch (e) {
      setError(`No se pudo validar el canal: ${e}`);
    } finally {
      setPlayingChannelUrl(null);
    }
  }

  function playItem(item: CombinedItem) {
    if (item.kind === "media" && item.media) {
      onPlayMedia(item.media);
      onClose();
    } else if (item.kind === "channel" && item.channel) {
      playChannel(item.channel);
    } else if (item.kind === "youtube" && item.video) {
      onPlayYoutube({ videoId: item.video.video_id, title: item.video.title });
      onClose();
    } else if (item.kind === "online" && item.online) {
      playOnlineItem(item.online);
    }
  }

  // Mismo flujo que OnlineLibraryTab.view(): si ya está en la colección
  // reproduce directo, si no lo agrega primero (identifier tal cual, sin
  // transformar — ver add_online_item_inner en el backend).
  async function playOnlineItem(item: OnlineItem) {
    const existing = mediaItems.find((m) => m.source_identifier === item.identifier);
    if (existing) {
      onPlayMedia(existing);
      onClose();
      return;
    }
    setOnlineBusyKey(`${item.kind}:${item.identifier}`);
    setError(null);
    try {
      const info = await api.addOnlineItem(item.kind, item.identifier, item.title, item.year, item.license);
      const url = await api.getStreamUrl(info.id, 0);
      onPlayOnline(item.title, url);
      onMediaAdded();
      onClose();
    } catch (e) {
      setError(`No se pudo reproducir "${item.title}": ${e}`);
    } finally {
      setOnlineBusyKey(null);
    }
  }

  function isBusy(item: CombinedItem): boolean {
    if (item.kind === "channel") return playingChannelUrl === item.channel?.url;
    if (item.kind === "online" && item.online) return onlineBusyKey === `${item.online.kind}:${item.online.identifier}`;
    return false;
  }

  function playYoutubeVideo(video: YoutubeVideo) {
    onPlayYoutube({ videoId: video.video_id, title: video.title });
    onClose();
  }

  // Mismo bug real reportado en vivo para AddTorrentModal.IndexersTab.add():
  // reagregar un magnet ya presente en la colección hacía que el motor lo
  // procesara de cero (verificación de piezas incluida) en vez de solo
  // reproducirlo.
  async function addIndexerResult(result: IndexerResult) {
    const existing = mediaItems.find((m) => m.source_identifier === result.magnet);
    if (existing) {
      onPlayMedia(existing);
      onClose();
      return;
    }
    setAddingMagnet(result.magnet);
    setError(null);
    try {
      await api.addTorrent(result.magnet);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setAddingMagnet(null);
    }
  }

  // Mismo trigger opt-in por fila que ya tiene AddTorrentModal.IndexersTab
  // (pedido explícito del usuario: salud también en la lupa, no solo en "+").
  async function checkIndexerHealth(magnet: string) {
    setCheckingHealthMagnet(magnet);
    try {
      const [h] = await api.checkTorrentHealthBatch([magnet]);
      setIndexerHealth((prev) => ({ ...prev, [magnet]: h }));
    } catch (e) {
      setError(String(e));
    } finally {
      setCheckingHealthMagnet(null);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-zinc-200 bg-white shadow-xl dark:border-zinc-800 dark:bg-zinc-900"
      >
        <div className="flex items-center gap-2 border-b border-zinc-200 px-3 py-2 dark:border-zinc-800">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4 shrink-0 text-zinc-400">
            <circle cx="11" cy="11" r="7" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && tab === "todo" && searchWithAi()}
            placeholder="Buscar…"
            className="flex-1 bg-transparent text-sm outline-none dark:text-zinc-100"
          />
          <button onClick={onClose} className="shrink-0 rounded-md p-1.5 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Mismo estilo de tira de pestañas que AddSourceModal — cada
            categoría muestra solo lo suyo, sin un toggle aparte que mezcle
            qué se filtra con qué se busca en profundidad (confuso,
            reportado en vivo). */}
        <div className="flex gap-1 border-b border-zinc-200 bg-slate-50 px-3 pt-2 dark:border-zinc-800 dark:bg-zinc-950">
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

        {error && <p className="px-3 pt-2 text-xs text-red-500">{error}</p>}

        <div className="flex-1 overflow-y-auto p-3">
          {loading && <p className="p-4 text-center text-xs text-zinc-500 dark:text-zinc-400">Cargando…</p>}

          {!loading && tab === "todo" && (
            <div className="flex flex-col gap-3">
              <div className="flex items-center gap-2">
                <button
                  onClick={searchWithAi}
                  disabled={aiLoading || !query.trim()}
                  className="rounded-md border border-[var(--accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
                >
                  {aiLoading ? "Buscando con IA…" : "Buscar con IA"}
                </button>
                <span className="text-[10px] text-zinc-500 dark:text-zinc-400">
                  Rankea lo ya agregado por significado, no solo coincidencia de texto.
                </span>
              </div>

              {aiLocalResults === null && (
                <>
                  {filtered.length === 0 && (
                    <p className="p-4 text-center text-xs text-zinc-500 dark:text-zinc-400">
                      {query.trim() ? "Sin resultados — prueba \"Buscar con IA\", o busca en una categoría específica." : "Escribe para filtrar."}
                    </p>
                  )}
                  <ul className="flex flex-col gap-1">
                    {filtered.map((item) => (
                      <ResultRow key={item.key} item={item} busy={isBusy(item)} onClick={() => playItem(item)} />
                    ))}
                  </ul>
                </>
              )}

              {aiLocalResults !== null && (
                <>
                  {aiLocalResults.length === 0 && !aiLoading && (
                    <p className="p-4 text-center text-xs text-zinc-500 dark:text-zinc-400">
                      Nada encontrado con IA en lo ya agregado.
                    </p>
                  )}
                  <ul className="flex flex-col gap-1">
                    {aiLocalResults.map((item) => (
                      <ResultRow key={item.key} item={item} busy={isBusy(item)} onClick={() => playItem(item)} />
                    ))}
                  </ul>
                </>
              )}
            </div>
          )}

          {!loading && tab === "torrents" && (
            <div className="flex flex-col gap-3">
              <div className="flex items-center gap-2">
                <button
                  onClick={searchArchiveOrgLive}
                  disabled={searchingArchiveOrg || !query.trim()}
                  className="rounded-md border border-[var(--accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
                >
                  {searchingArchiveOrg ? "Buscando en archive.org…" : "Buscar en archive.org"}
                </button>
                <span className="text-[10px] text-zinc-500 dark:text-zinc-400">
                  Catálogo legal completo, más allá de lo ya cargado abajo.
                </span>
              </div>

              {archiveOrgResults !== null && (
                <ResultGroup title="Resultados de archive.org">
                  {archiveOrgResults.length === 0 && (
                    <p className="p-2 text-xs text-zinc-500 dark:text-zinc-400">Sin resultados.</p>
                  )}
                  {archiveOrgResults.map((item) => (
                    <li key={item.identifier}>
                      <button
                        onClick={() => addArchiveOrgResult(item)}
                        disabled={addingArchiveOrgId === item.identifier}
                        className="flex w-full items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs disabled:opacity-50 dark:border-zinc-800"
                      >
                        <span className="min-w-0 truncate text-zinc-900 dark:text-zinc-100">{item.title}</span>
                        <span className="shrink-0 text-[10px] text-[var(--accent)] dark:text-[var(--accent-fg)]">
                          {addingArchiveOrgId === item.identifier ? "Agregando…" : "Agregar"}
                        </span>
                      </button>
                    </li>
                  ))}
                </ResultGroup>
              )}

              <ResultGroup title="Ya cargado (Online + Public Domain Torrents)">
                {filteredOnline.length === 0 && (
                  <p className="p-2 text-xs text-zinc-500 dark:text-zinc-400">Sin resultados.</p>
                )}
                {filteredOnline.map((o) => {
                  const key = `${o.kind}:${o.identifier}`;
                  return (
                    <li key={key}>
                      <button
                        onClick={() => playOnlineItem(o)}
                        disabled={onlineBusyKey === key}
                        className="flex w-full items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs disabled:cursor-wait disabled:opacity-50 dark:border-zinc-800"
                      >
                        <span className="min-w-0 truncate text-zinc-900 dark:text-zinc-100">{o.title}</span>
                        <span className="shrink-0 text-[10px] text-zinc-500">
                          {onlineBusyKey === key ? "Validando…" : "Online"}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ResultGroup>
            </div>
          )}

          {!loading && tab === "indexers" && (
            <div className="flex flex-col gap-3">
              <div className="flex items-center gap-2">
                <button
                  onClick={searchMyIndexers}
                  disabled={searchingIndexers || !query.trim()}
                  className="rounded-md border border-[var(--accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
                >
                  {searchingIndexers ? (aiSearch ? "Buscando con IA…" : "Buscando…") : "Buscar en mis indexers"}
                </button>
                <span className="text-[10px] text-zinc-500 dark:text-zinc-400">
                  Popcorn no trae ninguno precargado — configúralos en Ajustes → Indexers.
                </span>
              </div>
              <label className="flex items-center gap-1.5 text-[11px] text-zinc-600 dark:text-zinc-300">
                <input type="checkbox" checked={aiSearch} onChange={(e) => toggleAiSearch(e.target.checked)} />
                Búsqueda ampliada con IA (más lenta — prueba términos alternativos, otros idiomas)
              </label>

              {indexerResults !== null && (
                <>
                  {indexerResults.length === 0 && (
                    <p className="p-4 text-center text-xs text-zinc-500 dark:text-zinc-400">
                      Sin resultados, o no tienes indexers agregados — configúralos en Ajustes → Indexers.
                    </p>
                  )}
                  <ul className="flex flex-col gap-1">
                    {indexerResults.map((r) => {
                      const h = indexerHealth[r.magnet];
                      const tags = parseTorrentTags(r.title);
                      return (
                        <li
                          key={r.magnet}
                          className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-xs dark:border-zinc-800"
                        >
                          <button
                            onClick={() => addIndexerResult(r)}
                            disabled={addingMagnet === r.magnet}
                            className="flex min-w-0 flex-1 flex-col items-start gap-0.5 text-left disabled:opacity-50"
                          >
                            <span className="min-w-0 truncate text-zinc-900 dark:text-zinc-100">{r.title}</span>
                            {tags.length > 0 && (
                              <span className="flex flex-wrap gap-1">
                                {tags.map((t) => (
                                  <span
                                    key={t}
                                    className="rounded-full bg-zinc-100 px-1.5 py-0.5 text-[10px] text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300"
                                  >
                                    {t}
                                  </span>
                                ))}
                              </span>
                            )}
                            {h && h.peers_found !== null && (
                              <span className="text-[10px] font-medium text-emerald-600 dark:text-emerald-400">
                                {h.peers_found} peers reales ({h.source})
                              </span>
                            )}
                            {h && h.peers_found === null && (
                              <span className="text-[10px] text-zinc-400">salud: sin datos</span>
                            )}
                          </button>
                          {!h && (
                            <button
                              onClick={() => checkIndexerHealth(r.magnet)}
                              disabled={checkingHealthMagnet === r.magnet}
                              title="Consulta tracker/DHT reales — puede tardar varios segundos"
                              className="shrink-0 rounded-full border border-zinc-300 px-1.5 py-0.5 text-[10px] text-zinc-500 hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
                            >
                              {checkingHealthMagnet === r.magnet ? "Consultando…" : "Verificar salud"}
                            </button>
                          )}
                          <span className="shrink-0 text-[10px] text-[var(--accent)] dark:text-[var(--accent-fg)]">
                            {addingMagnet === r.magnet ? "Agregando…" : "Agregar"}
                          </span>
                        </li>
                      );
                    })}
                  </ul>
                </>
              )}
            </div>
          )}

          {!loading && tab === "iptv" && (
            <ul className="flex flex-col gap-1">
              {filteredChannels.length === 0 && (
                <p className="p-4 text-center text-xs text-zinc-500 dark:text-zinc-400">Sin resultados.</p>
              )}
              {filteredChannels.map((c) => (
                <li key={`${c.source_id}:${c.url}`}>
                  <button
                    onClick={() => playChannel(c)}
                    disabled={playingChannelUrl === c.url}
                    className="flex w-full items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs disabled:cursor-wait disabled:opacity-50 dark:border-zinc-800"
                  >
                    <span className="min-w-0 truncate text-zinc-900 dark:text-zinc-100">{c.name}</span>
                    <span className="shrink-0 text-[10px] text-zinc-500">
                      {playingChannelUrl === c.url ? "Validando…" : "IPTV"}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}

          {!loading && tab === "youtube" && (
            <div className="flex flex-col gap-3">
              <ul className="flex flex-col gap-1">
                {filteredVideos.length === 0 && (
                  <p className="p-2 text-xs text-zinc-500 dark:text-zinc-400">Sin resultados en lo ya cargado.</p>
                )}
                {filteredVideos.map((v) => (
                  <li key={`${v.source_id}:${v.video_id}`}>
                    <button
                      onClick={() => playYoutubeVideo(v)}
                      className="flex w-full items-center gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs dark:border-zinc-800"
                    >
                      <span className="min-w-0 truncate text-zinc-900 dark:text-zinc-100">{v.title}</span>
                    </button>
                  </li>
                ))}
              </ul>

              <div className="flex items-center gap-2">
                <button
                  onClick={searchYoutubeDeep}
                  disabled={searchingYoutubeDeep || !query.trim()}
                  className="rounded-md border border-[var(--accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
                >
                  {searchingYoutubeDeep ? "Buscando…" : "Buscar más allá de lo cargado"}
                </button>
              </div>

              {youtubeDeepResults !== null && (
                <ResultGroup title="Más allá de lo cargado">
                  {youtubeDeepResults.length === 0 && (
                    <p className="p-2 text-xs text-zinc-500 dark:text-zinc-400">Sin resultados.</p>
                  )}
                  {youtubeDeepResults.map((v) => (
                    <li key={`${v.source_id}:${v.video_id}`}>
                      <button
                        onClick={() => playYoutubeVideo(v)}
                        className="flex w-full items-center gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs dark:border-zinc-800"
                      >
                        <span className="min-w-0 truncate text-zinc-900 dark:text-zinc-100">{v.title}</span>
                      </button>
                    </li>
                  ))}
                </ResultGroup>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function ResultGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <p className="text-[10px] font-semibold uppercase tracking-wide text-zinc-400">{title}</p>
      <ul className="flex flex-col gap-1">{children}</ul>
    </div>
  );
}

function ResultRow({ item, busy, onClick }: { item: CombinedItem; busy: boolean; onClick: () => void }) {
  return (
    <li>
      <button
        onClick={onClick}
        disabled={busy}
        className="flex w-full items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs disabled:cursor-wait disabled:opacity-50 dark:border-zinc-800"
      >
        <span className="min-w-0 truncate text-zinc-900 dark:text-zinc-100">{item.title}</span>
        <span className="shrink-0 rounded-full bg-[var(--accent-soft)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--accent)] dark:text-[var(--accent-fg)]">
          {busy ? "Validando…" : KIND_LABEL[item.kind]}
        </span>
      </button>
    </li>
  );
}
