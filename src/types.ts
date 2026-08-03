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
  finished: boolean;
  state: "initializing" | "live" | "paused" | "error";
  error: string | null;
}

export interface ArchiveOrgItem {
  identifier: string;
  title: string;
  year: number | null;
  licenseurl: string | null;
}

export interface MediaItem {
  id: string;
  source_type: "archive_org" | "magnet" | "torrent_file";
  source_identifier: string;
  title: string;
  year: number | null;
  license: string | null;
  engine_torrent_id: string | null;
  is_private: boolean;
  added_at: string;
}
