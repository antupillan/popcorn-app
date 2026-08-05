import { invoke } from "@tauri-apps/api/core";
import type { ArchiveOrgItem, Channel, IptvSource, MediaItem, RecordingInfo, TorrentInfo } from "../types";

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

  startRecording: (sourceId: string | null, channelName: string, manifestUrl: string) =>
    invoke<RecordingInfo>("start_recording", { sourceId, channelName, manifestUrl }),

  stopRecording: (id: string) => invoke<void>("stop_recording", { id }),

  listRecordings: () => invoke<RecordingInfo[]>("list_recordings"),
};
