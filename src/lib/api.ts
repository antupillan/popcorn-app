import { invoke } from "@tauri-apps/api/core";
import type {
  ArchiveOrgItem,
  Channel,
  IptvSource,
  LocalFile,
  MediaItem,
  OnlineItem,
  RecordingInfo,
  TorrentInfo,
} from "../types";

// Envoltorios finos y tipados sobre invoke() — un lugar único por comando,
// para no repetir el nombre del comando ni el tipo de retorno en cada
// componente que lo necesite.

export const api = {
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

  // Rápida (archive.org + Blender Foundation, ~1.5s medido en vivo) —
  // separada de browsePublicDomainTorrents (ese sitio de terceros tarda
  // ~8s) para que el frontend pueda renderizar cada grupo apenas responde
  // en vez de esperar al más lento.
  browseOnlineLibrary: () => invoke<OnlineItem[]>("browse_online_library"),

  browsePublicDomainTorrents: () => invoke<OnlineItem[]>("browse_public_domain_torrents"),

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
};
