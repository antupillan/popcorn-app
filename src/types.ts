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

export interface YoutubeSource {
  id: string;
  name: string;
  channel_url: string;
  channel_id: string | null;
  category: "cine" | "series" | "anime" | "musica";
  enabled: boolean;
}

export interface YoutubeVideo {
  video_id: string;
  title: string;
  published_at: string;
  thumbnail_url: string | null;
  source_id: string;
  category: "cine" | "series" | "anime" | "musica";
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

export interface AiProviderConfig {
  id: string;
  kind: "gemini" | "openai_compatible";
  label: string;
  model: string;
  base_url: string | null;
  active: boolean;
  has_api_key: boolean;
}

export interface JsonPaths {
  items_path: string;
  title_field: string;
  magnet_field: string;
  size_field: string | null;
  seeders_field: string | null;
}

export interface Indexer {
  id: string;
  name: string;
  search_url_template: string;
  result_format: "magnet_list" | "rss" | "json";
  json_paths: JsonPaths | null;
  enabled: boolean;
}

export interface IndexerResult {
  title: string;
  magnet: string;
  size: string | null;
  seeders: string | null;
  source_indexer: string;
}

export interface IndexerFailure {
  indexer_name: string;
  message: string;
}

export interface SearchIndexersResponse {
  results: IndexerResult[];
  errors: IndexerFailure[];
}

export interface TorrentHealth {
  peers_found: number | null;
  source: "tracker" | "dht" | "sin datos";
}

export interface ScoredCandidate {
  index: number;
  score: number;
}

export interface Subtitle {
  id: string;
  media_item_id: string;
  language: string;
  origin: "original" | "ai_translated" | "human_edited";
  content: string;
  created_at: string;
}

export interface OpenSubtitlesResult {
  file_id: number;
  file_name: string;
}

export interface SourceSettings {
  id: string;
  label: string;
  curation_enabled: boolean;
  mediatype_filter: string | null;
}

export interface SpeedLimits {
  upload_kbps: number | null;
  download_kbps: number | null;
}

export interface TorrentEngineConfig {
  kind: "embedded" | "qbittorrent";
  qbittorrent_base_url: string | null;
  qbittorrent_username: string | null;
  qbittorrent_has_password: boolean;
}
