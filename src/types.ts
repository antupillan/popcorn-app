// Espejo de los structs #[derive(Serialize)] en src-tauri/src/{engine,commands,sources}.
// Mantener sincronizado a mano — son pocos campos y cambian junto al backend.

export interface TorrentInfo {
  id: string;
  name: string | null;
  info_hash: string;
  progress_bytes: number;
  total_bytes: number;
  download_speed_mbps: number;
  upload_speed_mbps: number;
  uploaded_bytes: number;
  finished: boolean;
  state: "initializing" | "live" | "paused" | "error";
  error: string | null;
}

export interface ArchiveOrgItem {
  identifier: string;
  title: string;
  year: number | null;
  licenseurl: string | null;
  thumbnail_url: string;
}

export interface MediaItem {
  id: string;
  source_type: "archive_org" | "magnet" | "torrent_file" | "public_domain_torrents";
  source_identifier: string;
  title: string;
  year: number | null;
  license: string | null;
  engine_torrent_id: string | null;
  is_private: boolean;
  added_at: string;
}

export interface OnlineItem {
  kind: "archive_org" | "public_domain_torrents" | "blender_foundation" | "prelinger" | "feature_films";
  identifier: string;
  title: string;
  year: number | null;
  license: string | null;
  thumbnail_url: string | null;
}

export interface LocalFile {
  path: string;
  name: string;
}

export interface IptvSource {
  id: string;
  name: string;
  source_kind: "url" | "file";
  playlist_url: string | null;
  enabled: boolean;
}

export interface Channel {
  name: string;
  url: string;
  group: string | null;
  logo_url: string | null;
  tvg_id: string | null;
  source_id: string;
}

export interface RecordingInfo {
  id: string;
  source_id: string | null;
  channel_name: string;
  manifest_url: string;
  file_name: string;
  status: "recording" | "stopped" | "error";
  error: string | null;
  bytes_written: number;
  started_at: string;
  stopped_at: string | null;
}
