import { invoke } from "@tauri-apps/api/core";
import type { ArchiveOrgItem, MediaItem, TorrentInfo } from "../types";

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
};
