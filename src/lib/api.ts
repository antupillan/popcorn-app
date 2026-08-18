import { invoke } from "@tauri-apps/api/core";
import type {
  AiProviderConfig,
  ArchiveOrgItem,
  Channel,
  Indexer,
  IndexerResult,
  IptvSource,
  JsonPaths,
  LocalFile,
  MediaItem,
  OnlineItem,
  RecordingInfo,
  OpenSubtitlesResult,
  ScoredCandidate,
  SourceSettings,
  SpeedLimits,
  Subtitle,
  TorrentEngineConfig,
  TorrentHealth,
  TorrentInfo,
  YoutubeSource,
  YoutubeVideo,
} from "../types";

// Envoltorios finos y tipados sobre invoke() — un lugar único por comando,
// para no repetir el nombre del comando ni el tipo de retorno en cada
// componente que lo necesite.

export const api = {
  getOs: () => invoke<string>("get_os"),

  getOsAccentColor: () => invoke<string | null>("get_os_accent_color"),

  setWindowEffectsEnabled: (enabled: boolean) => invoke<void>("set_window_effects_enabled", { enabled }),

  // data: URIs de window-close/minimize/maximize/restore del tema de
  // íconos real del usuario — solo Linux (native_icons.rs), objeto vacío
  // en cualquier otro caso, ver TitleBar.tsx para el fallback dibujado.
  getNativeWindowIcons: () => invoke<Record<string, string>>("get_native_window_icons"),

  // `Some(mensaje)` solo si el motor externo configurado no pudo conectar
  // al arrancar y la app cayó al motor embebido — ver lib.rs::setup.
  getEngineFallbackWarning: () => invoke<string | null>("get_engine_fallback_warning"),

  addTorrent: (magnet: string) => invoke<TorrentInfo>("add_torrent", { magnet }),

  listTorrents: () => invoke<TorrentInfo[]>("list_torrents"),

  pauseTorrent: (id: string) => invoke<void>("pause_torrent", { id }),

  removeTorrent: (id: string, deleteFiles: boolean) =>
    invoke<void>("remove_torrent", { id, deleteFiles }),

  getStreamUrl: (id: string, fileIdx: number) =>
    invoke<string>("get_stream_url", { id, fileIdx }),

  // Usa media_items (id de biblioteca, no engine_torrent_id) como fuente de
  // verdad — sana el torrent si la sesión del motor lo perdió al reiniciar
  // la app (bug #26, EmbeddedRqbit no persiste sesión entre reinicios).
  getStreamUrlForMediaItem: (mediaId: string, fileIdx: number) =>
    invoke<string>("get_stream_url_for_media_item", { mediaId, fileIdx }),

  searchArchiveOrg: (query: string) =>
    invoke<ArchiveOrgItem[]>("search_archive_org", { query }),

  addArchiveOrgItem: (item: ArchiveOrgItem) =>
    invoke<TorrentInfo>("add_archive_org_item", {
      identifier: item.identifier,
      title: item.title,
      year: item.year,
      licenseurl: item.licenseurl,
    }),

  listMediaItems: () => invoke<MediaItem[]>("list_media_items"),

  removeMediaItem: (id: string) => invoke<void>("remove_media_item", { id }),

  addTorrentFile: (bytes: number[]) => invoke<TorrentInfo>("add_torrent_file", { bytes }),

  listIptvSources: () => invoke<IptvSource[]>("list_iptv_sources"),

  addIptvSourceUrl: (name: string, playlistUrl: string) =>
    invoke<IptvSource>("add_iptv_source_url", { name, playlistUrl }),

  addIptvSourceFile: (name: string, bytes: number[]) =>
    invoke<IptvSource>("add_iptv_source_file", { name, bytes }),

  removeIptvSource: (id: string) => invoke<void>("remove_iptv_source", { id }),

  toggleIptvSource: (id: string, enabled: boolean) =>
    invoke<void>("toggle_iptv_source", { id, enabled }),

  listChannels: () => invoke<Channel[]>("list_channels"),

  // Re-cura una lista ya obtenida — se llama después de renderizar el
  // resultado rápido sin curar, nunca antes (mismo criterio que
  // curateOnlineLibrary, ver online_library.rs).
  curateChannels: (channels: Channel[]) => invoke<Channel[]>("curate_channels", { channels }),

  // "Supervisión liviana" (ver plan IPTV): Rust valida/reintenta el
  // manifest inicial y devuelve la URL final post-redirect; hls.js pide los
  // segmentos directo contra esa URL, sin pasar por el backend.
  validateChannelManifest: (url: string) =>
    invoke<string>("validate_channel_manifest", { url }),

  startRecording: (
    sourceId: string | null,
    channelName: string,
    manifestUrl: string,
    maxDurationMinutes: number | null,
  ) => invoke<RecordingInfo>("start_recording", { sourceId, channelName, manifestUrl, maxDurationMinutes }),

  stopRecording: (id: string) => invoke<void>("stop_recording", { id }),

  listRecordings: () => invoke<RecordingInfo[]>("list_recordings"),

  getRecordingStreamUrl: (id: string) => invoke<string>("get_recording_stream_url", { id }),

  deleteRecording: (id: string) => invoke<void>("delete_recording", { id }),

  listYoutubeSources: () => invoke<YoutubeSource[]>("list_youtube_sources"),

  addYoutubeSource: (name: string, channelUrl: string, category: string) =>
    invoke<YoutubeSource>("add_youtube_source", { name, channelUrl, category }),

  removeYoutubeSource: (id: string) => invoke<void>("remove_youtube_source", { id }),

  toggleYoutubeSource: (id: string, enabled: boolean) =>
    invoke<void>("toggle_youtube_source", { id, enabled }),

  listYoutubeVideos: () => invoke<YoutubeVideo[]>("list_youtube_videos"),

  // Mismo criterio que curateChannels: se llama después del render rápido
  // sin curar, nunca antes.
  curateYoutubeVideos: (videos: YoutubeVideo[]) =>
    invoke<YoutubeVideo[]>("curate_youtube_videos", { videos }),

  setYoutubeApiKey: (key: string) => invoke<void>("set_youtube_api_key", { key }),

  getYoutubeApiKeyStatus: () => invoke<boolean>("get_youtube_api_key_status"),

  removeYoutubeApiKey: () => invoke<void>("remove_youtube_api_key"),

  // Búsqueda global (lupa): más allá del tope MAX_VIDEOS_PER_SOURCE por
  // canal ya agregado, nunca contra canales nuevos (ver
  // Planes_mejora_popcorn/busqueda_global.txt).
  searchYoutubeVideosInAddedChannels: (query: string) =>
    invoke<YoutubeVideo[]>("search_youtube_videos_in_added_channels", { query }),

  // Rápida (archive.org + Blender Foundation, ~1.5s medido en vivo) —
  // separada de browsePublicDomainTorrents (ese sitio de terceros tarda
  // ~8s) para que el frontend pueda renderizar cada grupo apenas responde
  // en vez de esperar al más lento.
  browseOnlineLibrary: () => invoke<OnlineItem[]>("browse_online_library"),

  browsePublicDomainTorrents: () => invoke<OnlineItem[]>("browse_public_domain_torrents"),

  // Re-cura una lista ya obtenida — se llama después de renderizar el
  // resultado rápido sin curar, nunca antes (ver online_library.rs:
  // curar síncrono en el fetch inicial podía tardar minutos con un
  // proveedor de IA inalcanzable).
  curateOnlineLibrary: (items: OnlineItem[]) => invoke<OnlineItem[]>("curate_online_library", { items }),

  addOnlineItem: (
    kind: string,
    identifier: string,
    title: string,
    year: number | null,
    license: string | null,
  ) => invoke<TorrentInfo>("add_online_item", { kind, identifier, title, year, license }),

  // Sembrado real (solo familia archive.org: archive_org/blender_foundation/
  // prelinger/feature_films) — descarga el .torrent completo antes de
  // agregarlo, a diferencia de addOnlineItem/"Ver" que reproduce vía proxy
  // sin esperar. Puede tardar bastante más que "Ver" en ítems grandes.
  seedArchiveOrgItem: (
    identifier: string,
    title: string,
    year: number | null,
    licenseurl: string | null,
  ) => invoke<TorrentInfo>("seed_archive_org_item", { identifier, title, year, licenseurl }),

  getLocalLibraryFolder: () => invoke<string | null>("get_local_library_folder"),

  setLocalLibraryFolder: (folder: string) => invoke<void>("set_local_library_folder", { folder }),

  listLocalFiles: () => invoke<LocalFile[]>("list_local_files"),

  getLocalStreamUrl: (path: string) => invoke<string>("get_local_stream_url", { path }),

  listAiProviders: () => invoke<AiProviderConfig[]>("list_ai_providers"),

  // La api_key nunca vuelve del backend (ver AiProviderConfig.has_api_key)
  // — solo se envía al crear, va directo al keychain del SO.
  addAiProvider: (
    kind: string,
    label: string,
    model: string,
    baseUrl: string | null,
    apiKey: string | null,
  ) => invoke<AiProviderConfig>("add_ai_provider", { kind, label, model, baseUrl, apiKey }),

  removeAiProvider: (id: string) => invoke<void>("remove_ai_provider", { id }),

  setActiveAiProvider: (id: string) => invoke<void>("set_active_ai_provider", { id }),

  listIndexers: () => invoke<Indexer[]>("list_indexers"),

  addIndexer: (
    name: string,
    searchUrlTemplate: string,
    resultFormat: string,
    jsonPaths: JsonPaths | null,
  ) => invoke<Indexer>("add_indexer", { name, searchUrlTemplate, resultFormat, jsonPaths }),

  removeIndexer: (id: string) => invoke<void>("remove_indexer", { id }),

  toggleIndexer: (id: string, enabled: boolean) => invoke<void>("toggle_indexer", { id, enabled }),

  testIndexer: (indexer: Indexer, query: string) =>
    invoke<IndexerResult[]>("test_indexer", { indexer, query }),

  // Busca contra todos los indexers habilitados y fusiona resultados ya
  // curados (curation_enabled por indexer, ver source_settings) — a
  // diferencia de testIndexer, que valida uno solo al configurarlo.
  searchIndexers: (query: string) => invoke<IndexerResult[]>("search_indexers", { query }),
  searchIndexersWithAi: (query: string) => invoke<IndexerResult[]>("search_indexers_with_ai", { query }),

  // Opt-in, nunca automático (a diferencia de la curación IA) — consulta
  // tracker UDP + DHT reales, más lento que el `seeders` que ya trae el
  // indexer. Preserva el orden de `magnets` para mergear por índice.
  checkTorrentHealthBatch: (magnets: string[]) =>
    invoke<TorrentHealth[]>("check_torrent_health_batch", { magnets }),

  // Búsqueda global (lupa): rankea `candidates` (títulos de lo ya
  // agregado) contra `query` vía curate_by_hint del proveedor de IA
  // activo — sin caché, la lista cambia todo el tiempo. Falla explícito
  // sin proveedor activo.
  searchAddedContentWithAi: (query: string, candidates: string[]) =>
    invoke<ScoredCandidate[]>("search_added_content_with_ai", { query, candidates }),

  listSubtitles: (mediaItemId: string) => invoke<Subtitle[]>("list_subtitles", { mediaItemId }),

  addSubtitleText: (mediaItemId: string, language: string, origin: string, content: string) =>
    invoke<Subtitle>("add_subtitle_text", { mediaItemId, language, origin, content }),

  removeSubtitle: (id: string) => invoke<void>("remove_subtitle", { id }),

  // Falla explícito sin proveedor de IA activo — nunca degrada en silencio
  // a "sin traducción".
  translateSubtitleTexts: (texts: string[], targetLang: string) =>
    invoke<string[]>("translate_subtitle_texts", { texts, targetLang }),

  searchOpensubtitles: (query: string, language: string) =>
    invoke<OpenSubtitlesResult[]>("search_opensubtitles", { query, language }),

  downloadOpensubtitlesSubtitle: (fileId: number) =>
    invoke<string>("download_opensubtitles_subtitle", { fileId }),

  setOpensubtitlesCredentials: (username: string, password: string) =>
    invoke<void>("set_opensubtitles_credentials", { username, password }),

  getOpensubtitlesCredentialsStatus: () => invoke<boolean>("get_opensubtitles_credentials_status"),

  removeOpensubtitlesCredentials: () => invoke<void>("remove_opensubtitles_credentials"),

  listSourceSettings: () => invoke<SourceSettings[]>("list_source_settings"),

  setSourceCurationEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_source_curation_enabled", { id, enabled }),

  getSpeedLimits: () => invoke<SpeedLimits>("get_speed_limits"),

  // Aplica solo a torrents agregados de acá en adelante — librqbit no
  // expone forma de reconfigurar uno ya corriendo (ver comentario en
  // engine/mod.rs::add_with_limits).
  setSpeedLimits: (uploadKbps: number | null, downloadKbps: number | null) =>
    invoke<void>("set_speed_limits", { uploadKbps, downloadKbps }),

  getTorrentEngineConfig: () => invoke<TorrentEngineConfig>("get_torrent_engine_config"),

  // La password nunca vuelve del backend (ver TorrentEngineConfig.qbittorrent_has_password)
  // — `null`/vacío deja la ya guardada tal cual, mismo criterio que addAiProvider.
  setTorrentEngineConfig: (
    kind: string,
    qbittorrentBaseUrl: string | null,
    qbittorrentUsername: string | null,
    qbittorrentPassword: string | null,
  ) =>
    invoke<void>("set_torrent_engine_config", {
      kind,
      qbittorrentBaseUrl,
      qbittorrentUsername,
      qbittorrentPassword,
    }),

  // No persiste nada — solo confirma login + una llamada real contra la
  // Web API, para probar antes de guardar.
  testTorrentEngine: (baseUrl: string, username: string, password: string) =>
    invoke<void>("test_torrent_engine", { baseUrl, username, password }),

  setTorrentProxyUrl: (url: string) => invoke<void>("set_torrent_proxy_url", { url }),

  getTorrentProxyStatus: () => invoke<boolean>("get_torrent_proxy_status"),

  removeTorrentProxyUrl: () => invoke<void>("remove_torrent_proxy_url"),

  setCatalogProxyUrl: (url: string) => invoke<void>("set_catalog_proxy_url", { url }),

  getCatalogProxyStatus: () => invoke<boolean>("get_catalog_proxy_status"),

  removeCatalogProxyUrl: () => invoke<void>("remove_catalog_proxy_url"),
};
