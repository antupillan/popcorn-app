use anyhow::Context;
use serde::{Deserialize, Serialize};

const SEARCH_URL: &str = "https://archive.org/advancedsearch.php";

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ArchiveOrgItem {
    pub identifier: String,
    /// archive.org devuelve `title` como lista en vez de string cuando el
    /// ítem tiene metadata con el campo repetido (confirmado en vivo contra
    /// las colecciones de feature_films, 2026-08-06: `ColorCrazinessTheThreeStooges`
    /// trae dos variantes del mismo título) — `deserialize_title` tolera
    /// ambas formas y se queda con la primera.
    #[serde(deserialize_with = "deserialize_title")]
    pub title: String,
    pub year: Option<i64>,
    pub licenseurl: Option<String>,
    /// No viene en la respuesta de advancedsearch.php — se deriva del
    /// `identifier` después de deserializar (ver `search`).
    #[serde(default)]
    pub thumbnail_url: String,
}

fn deserialize_title<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        One(String),
        Many(Vec<String>),
    }
    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::One(s) => Ok(s),
        StringOrVec::Many(v) => v
            .into_iter()
            .next()
            .ok_or_else(|| serde::de::Error::custom("title vacío")),
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    response: SearchResponseBody,
}

#[derive(Deserialize)]
struct SearchResponseBody {
    docs: Vec<ArchiveOrgItem>,
}

/// Only fuentes P2P nativas para v1 — archive.org distribuye vía .torrent
/// real (verificado contra la API en vivo), no scraping ni mirrors propios.
///
/// `mediatype_filter` (ej. "movies") restringe del lado del servidor de
/// archive.org, que indexa de todo (libros, audio, software) además de
/// video — sin esto, una búsqueda de película trae ruido no relacionado.
/// Viene de `source_settings.mediatype_filter`, nunca hardcodeado acá; si
/// es `None` no se aplica ningún filtro (fail-open, ver plan). `sort`
/// (ej. "downloads desc") lo usa `browse_movies` para el modo "browse sin
/// búsqueda" — `None` deja el orden por defecto de archive.org (relevancia,
/// no aplica sin query real de todos modos).
pub async fn search(
    client: &reqwest::Client,
    query: &str,
    mediatype_filter: Option<&str>,
    sort: Option<&str>,
) -> anyhow::Result<Vec<ArchiveOrgItem>> {
    let q = match mediatype_filter {
        Some(mt) if !mt.is_empty() => format!("({query}) AND mediatype:({mt})"),
        _ => query.to_string(),
    };
    let mut params = vec![
        ("q", q.as_str()),
        ("fl[]", "identifier"),
        ("fl[]", "title"),
        ("fl[]", "year"),
        ("fl[]", "licenseurl"),
        ("rows", "50"),
        ("output", "json"),
    ];
    if let Some(s) = sort {
        params.push(("sort[]", s));
    }
    let resp: SearchResponse =
        crate::http_retry::send_with_retry(|| client.get(SEARCH_URL).query(&params))
            .await
            .context("no se pudo contactar archive.org")?
            .json()
            .await
            .context("respuesta de archive.org con formato inesperado")?;
    let mut docs = resp.response.docs;
    for item in &mut docs {
        item.thumbnail_url = format!("https://archive.org/services/img/{}", item.identifier);
    }
    Ok(docs)
}

/// "Browse sin búsqueda" (ver plan, Biblioteca unificada): `*:*` + orden por
/// descargas trae ruido real incluso con `mediatype_filter` aplicado
/// (confirmado en vivo: "test file mp4", "graphics" genérico, guías de viaje
/// en los primeros resultados) — la curación por IA que aplica
/// `online_library::browse_online_library` sobre este resultado no es un
/// lujo, es lo que lo hace usable.
pub async fn browse_movies(
    client: &reqwest::Client,
    mediatype_filter: Option<&str>,
) -> anyhow::Result<Vec<ArchiveOrgItem>> {
    search(client, "*:*", mediatype_filter, Some("downloads desc")).await
}

/// Catálogo semilla verificado en vivo (formato reproducible + licencia CC
/// confirmados contra la API de metadata de archive.org): Blender
/// Foundation/Blender Studio ya distribuye estas películas por archive.org,
/// no hace falta un conector propio. `curation_enabled=0` en
/// `source_settings` para esta fuente (ver migración) — es una allowlist ya
/// vetted a mano, no pasa por `curate_by_hint`. Sin I/O: pura para poder
/// testearse sin red.
const BLENDER_FOUNDATION_ITEMS: &[(&str, &str, Option<i64>, Option<&str>)] = &[
    ("Sintel", "Sintel", Some(2010), Some("http://creativecommons.org/licenses/by/3.0/")),
    ("BigBuckBunny_124", "Big Buck Bunny", None, Some("http://creativecommons.org/licenses/by/3.0/")),
    ("ElephantsDream", "Elephants Dream", Some(2006), Some("http://creativecommons.org/licenses/by/3.0/us/")),
    (
        "tearsofsteelblendervfxopenmovie800p",
        "Tears of Steel",
        Some(2012),
        None,
    ),
    ("cosmos-laundromat", "Cosmos Laundromat", None, None),
];

/// Browse acotado a una colección de archive.org en vez de todo el sitio —
/// mismo mecanismo que `browse_movies` (mediatype:movies + orden por
/// descargas) pero con `collection:(...)` sumado del lado del servidor, que
/// reduce el ruido significativamente frente al browse sin acotar
/// (confirmado en vivo, ver plan). `collection_query` es query Lucene tal
/// cual — puede ser un solo id o varios unidos con OR.
async fn browse_collection(
    client: &reqwest::Client,
    collection_query: &str,
) -> anyhow::Result<Vec<ArchiveOrgItem>> {
    search(
        client,
        &format!("collection:({collection_query})"),
        Some("movies"),
        Some("downloads desc"),
    )
    .await
}

/// Colección Prelinger: films educativos/industriales/históricos de
/// dominio público curados por Rick Prelinger junto con Internet Archive —
/// 10.460 ítems reales confirmados en vivo contra la API (2026-08-06).
pub async fn browse_prelinger(client: &reqwest::Client) -> anyhow::Result<Vec<ArchiveOrgItem>> {
    browse_collection(client, "prelinger").await
}

/// La colección `feature_films` completa de archive.org tiene 28.407 ítems
/// reales, pero 17.552 (61%) están en `feature_films_unsorted` — un
/// grab-bag sin curar. Estas cuatro sub-colecciones (7.577 ítems, medido en
/// vivo 2026-08-06) son las que archive.org organiza por género real: cine
/// mudo, comedia, noir, sci-fi/horror.
const FEATURE_FILMS_COLLECTIONS: &str = "silent_films OR Comedy_Films OR Film_Noir OR SciFi_Horror";

pub async fn browse_feature_films(client: &reqwest::Client) -> anyhow::Result<Vec<ArchiveOrgItem>> {
    browse_collection(client, FEATURE_FILMS_COLLECTIONS).await
}

pub fn blender_foundation_items() -> Vec<ArchiveOrgItem> {
    BLENDER_FOUNDATION_ITEMS
        .iter()
        .map(|(identifier, title, year, licenseurl)| ArchiveOrgItem {
            identifier: identifier.to_string(),
            title: title.to_string(),
            year: *year,
            licenseurl: licenseurl.map(|s| s.to_string()),
            thumbnail_url: format!("https://archive.org/services/img/{identifier}"),
        })
        .collect()
}

#[derive(Deserialize)]
struct MetadataResponse {
    files: Vec<MetadataFile>,
}

#[derive(Deserialize)]
struct MetadataFile {
    name: String,
    format: Option<String>,
}

const PLAYABLE_FORMATS: &[&str] = &["h.264", "512kb mp4", "mpeg4"];

/// Fallback HTTP directo (ver TorrentEngine::register_http_fallback): la
/// mayoría de los .torrent de archive.org dependen de webseeds BEP19 que
/// librqbit no soporta (upstream: ikatson/rqbit#500), así que se resuelve
/// el archivo reproducible real vía la API de metadata pública y se
/// construye su URL de descarga directa, que sí soporta Range de forma
/// nativa (verificado contra la API en vivo).
pub async fn primary_video_file(
    client: &reqwest::Client,
    identifier: &str,
) -> anyhow::Result<String> {
    let url = format!("https://archive.org/metadata/{identifier}");
    let meta: MetadataResponse = crate::http_retry::send_with_retry(|| client.get(&url))
        .await
        .with_context(|| format!("no se pudo consultar {url}"))?
        .json()
        .await
        .context("metadata de archive.org con formato inesperado")?;

    let file = meta
        .files
        .iter()
        .find(|f| {
            f.format
                .as_deref()
                .map(|fmt| PLAYABLE_FORMATS.contains(&fmt.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .context("no se encontró un archivo de video reproducible en el ítem")?;

    Ok(format!(
        "https://archive.org/download/{identifier}/{}",
        file.name
    ))
}

/// URL del `.torrent` público de un ítem — una sola fuente de verdad,
/// reusada por `fetch_torrent_bytes` (descarga real) y por el ping de
/// disponibilidad de curación (`availability_ping.rs`: "disponible" para
/// esta familia de fuentes significa que este `.torrent` responde).
pub(crate) fn torrent_url(identifier: &str) -> String {
    format!("https://archive.org/download/{identifier}/{identifier}_archive.torrent")
}

/// Descarga el .torrent público del ítem. archive.org redirige (302) al
/// datanode real que lo sirve — reqwest sigue redirects por defecto.
pub async fn fetch_torrent_bytes(
    client: &reqwest::Client,
    identifier: &str,
) -> anyhow::Result<Vec<u8>> {
    let url = torrent_url(identifier);
    let resp = crate::http_retry::send_with_retry(|| client.get(&url))
        .await
        .with_context(|| format!("no se pudo descargar {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} respondió con error"))?;
    Ok(resp.bytes().await?.to_vec())
}

/// Un archivo dentro de un `.torrent` de archive.org, con su ruta de
/// destino real y el nombre con el que se descarga por HTTP.
pub struct TorrentFileTarget {
    /// Ruta relativa dentro de la carpeta de descargas del motor donde debe
    /// colocarse este archivo antes de agregar el torrent.
    pub local_path: std::path::PathBuf,
    /// Nombre tal cual lo expone la API de metadata de archive.org (sin
    /// sub-carpeta) — construye la URL de descarga directa:
    /// `https://archive.org/download/{identifier}/{archive_org_filename}`.
    pub archive_org_filename: String,
}

/// Resuelve, para cada archivo real dentro de un `.torrent`, la ruta
/// relativa exacta (dentro de la carpeta de descargas del motor) donde
/// tiene que colocarse — parseado directo del bencode real
/// (`librqbit-core`), no asumido "single file torrent". Necesario para el
/// sembrado real: `librqbit` solo verifica hash contra disco una vez, al
/// agregar el torrent (sin API de recheck en 8.1.1, confirmado contra el
/// código fuente) — si un archivo no está ya en su path exacto en ese
/// momento, nunca se va a reconocer como completo después.
///
/// Devuelve TODOS los archivos, no solo el reproducible — hallazgo real
/// probando contra `cosmos-laundromat` (2026-08-07): el `.torrent` de
/// archive.org incluye archivos de metadata chicos (`_meta.xml`,
/// `_meta.sqlite`, ~21KB) además del video; descargar solo el video deja
/// el torrent con un puñado de piezas frontera incompletas para siempre
/// (nunca llega a 100%, sigue esperando P2P por el resto).
///
/// También encontrado probando: `librqbit` mete los torrents de 2+
/// archivos en una sub-carpeta con el nombre del torrent por defecto
/// (`Session::get_default_subfolder_for_torrent` — método privado, no
/// expuesto públicamente; la regla se replicó acá leyendo su código
/// fuente, no adivinada) — sin ese prefijo, los archivos quedaban en el
/// nivel equivocado.
pub fn resolve_torrent_files(torrent_bytes: &[u8]) -> anyhow::Result<Vec<TorrentFileTarget>> {
    let meta = librqbit_core::torrent_metainfo::torrent_from_bytes::<buffers::ByteBufOwned>(torrent_bytes)
        .context("no se pudo parsear el .torrent")?;
    let files: Vec<_> = meta
        .info
        .iter_file_details()
        .context("estructura de archivos del .torrent inválida")?
        .collect();

    let subfolder = if files.len() >= 2 {
        meta.info.name.as_ref().and_then(|n| {
            let s = String::from_utf8_lossy(n.as_ref()).into_owned();
            (!s.is_empty()).then_some(s)
        })
    } else {
        None
    };

    files
        .iter()
        .map(|file| {
            let rel = file
                .filename
                .to_pathbuf()
                .context("nombre de archivo inválido dentro del .torrent")?;
            let archive_org_filename = rel
                .file_name()
                .and_then(|f| f.to_str())
                .ok_or_else(|| anyhow::anyhow!("archivo sin nombre válido dentro del .torrent"))?
                .to_string();
            let local_path = match &subfolder {
                Some(s) => std::path::PathBuf::from(s).join(&rel),
                None => rel,
            };
            Ok(TorrentFileTarget { local_path, archive_org_filename })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_item_with_plain_string_title() {
        let item: ArchiveOrgItem = serde_json::from_str(
            r#"{"identifier":"x","title":"Un Título Normal","year":2020,"licenseurl":null}"#,
        )
        .unwrap();
        assert_eq!(item.title, "Un Título Normal");
    }

    /// Caso real observado en vivo (2026-08-06,
    /// `ColorCrazinessTheThreeStooges` en la colección feature_films):
    /// archive.org devuelve `title` como lista cuando el ítem tiene el
    /// campo repetido en su metadata — sin este fallback, `search()` entero
    /// falla al deserializar el batch, no solo ese ítem.
    #[test]
    fn deserializes_item_with_title_as_array_taking_the_first_value() {
        let item: ArchiveOrgItem = serde_json::from_str(
            r#"{"identifier":"x","title":["Primero","Segundo"],"year":null,"licenseurl":null}"#,
        )
        .unwrap();
        assert_eq!(item.title, "Primero");
    }

    #[test]
    fn blender_foundation_items_returns_the_five_verified_titles() {
        let items = blender_foundation_items();
        assert_eq!(items.len(), 5);
        let identifiers: Vec<&str> = items.iter().map(|i| i.identifier.as_str()).collect();
        assert_eq!(
            identifiers,
            vec![
                "Sintel",
                "BigBuckBunny_124",
                "ElephantsDream",
                "tearsofsteelblendervfxopenmovie800p",
                "cosmos-laundromat",
            ]
        );
        assert_eq!(items[0].title, "Sintel");
        assert_eq!(items[0].year, Some(2010));
        assert_eq!(
            items[0].thumbnail_url,
            "https://archive.org/services/img/Sintel"
        );
    }

    /// Red real, deshabilitado por defecto. Confirma que los 5 identifiers
    /// siguen vivos y con formato reproducible en archive.org — una allowlist
    /// hardcodeada puede pudrirse si archive.org retira o renombra un ítem.
    #[tokio::test]
    #[ignore]
    async fn blender_foundation_items_are_still_live_and_playable() {
        let client = reqwest::Client::new();
        for item in blender_foundation_items() {
            primary_video_file(&client, &item.identifier)
                .await
                .unwrap_or_else(|e| panic!("{} ya no resuelve un archivo reproducible: {e}", item.identifier));
        }
    }

    /// Red real, deshabilitado por defecto. Confirma que `sort[]` es
    /// aceptado por la API real (no solo que el código compila).
    #[tokio::test]
    #[ignore]
    async fn browse_movies_returns_real_results_sorted_by_downloads() {
        let client = reqwest::Client::new();
        let items = browse_movies(&client, Some("movies")).await.unwrap();
        assert!(!items.is_empty());
    }

    /// Red real, deshabilitado por defecto. Confirma que la colección
    /// Prelinger sigue respondiendo con ítems reales.
    #[tokio::test]
    #[ignore]
    async fn browse_prelinger_returns_real_results() {
        let client = reqwest::Client::new();
        let items = browse_prelinger(&client).await.unwrap();
        assert!(!items.is_empty());
    }

    /// Red real, deshabilitado por defecto. Confirma que las cuatro
    /// sub-colecciones curadas de feature_films siguen respondiendo.
    #[tokio::test]
    #[ignore]
    async fn browse_feature_films_returns_real_results_from_curated_subcollections() {
        let client = reqwest::Client::new();
        let items = browse_feature_films(&client).await.unwrap();
        assert!(!items.is_empty());
    }

    // Construidos con los structs reales de librqbit-core + serializados con
    // librqbit-bencode, en vez de escribir bencode a mano — evita errores de
    // conteo de bytes en la longitud de cada string.
    mod resolve_torrent_files_tests {
        use super::*;
        use buffers::ByteBufOwned;
        use librqbit_core::torrent_metainfo::{TorrentMetaV1, TorrentMetaV1File, TorrentMetaV1Info};
        use librqbit_core::Id20;

        fn serialize(meta: &TorrentMetaV1<ByteBufOwned>) -> Vec<u8> {
            let mut buf = Vec::new();
            bencode::bencode_serialize_to_writer(meta, &mut buf).unwrap();
            buf
        }

        fn base_meta(info: TorrentMetaV1Info<ByteBufOwned>) -> TorrentMetaV1<ByteBufOwned> {
            TorrentMetaV1 {
                announce: Some(ByteBufOwned::from(b"http://example.org/ann".as_slice())),
                announce_list: vec![],
                info,
                comment: None,
                created_by: None,
                encoding: None,
                publisher: None,
                publisher_url: None,
                creation_date: None,
                info_hash: Id20::new([0u8; 20]),
            }
        }

        #[test]
        fn resolves_single_file_torrent_without_subfolder() {
            let info = TorrentMetaV1Info::<ByteBufOwned> {
                name: Some(ByteBufOwned::from(b"pelicula.mp4".as_slice())),
                pieces: ByteBufOwned::from(vec![0u8; 20]),
                piece_length: 16384,
                length: Some(123),
                files: None,
                ..Default::default()
            };
            let bytes = serialize(&base_meta(info));

            let files = resolve_torrent_files(&bytes).unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].local_path, std::path::PathBuf::from("pelicula.mp4"));
            assert_eq!(files[0].archive_org_filename, "pelicula.mp4");
        }

        #[test]
        fn resolves_multi_file_torrent_with_subfolder_for_every_file() {
            let info = TorrentMetaV1Info::<ByteBufOwned> {
                name: Some(ByteBufOwned::from(b"mi_item".as_slice())),
                pieces: ByteBufOwned::from(vec![0u8; 20]),
                piece_length: 16384,
                length: None,
                files: Some(vec![
                    TorrentMetaV1File {
                        length: 999,
                        path: vec![ByteBufOwned::from(b"mi_item_meta.xml".as_slice())],
                        attr: None,
                        sha1: None,
                        symlink_path: None,
                    },
                    TorrentMetaV1File {
                        length: 123,
                        path: vec![ByteBufOwned::from(b"pelicula.mp4".as_slice())],
                        attr: None,
                        sha1: None,
                        symlink_path: None,
                    },
                ]),
                ..Default::default()
            };
            let bytes = serialize(&base_meta(info));

            // 2+ archivos: librqbit antepone el nombre del torrent como
            // sub-carpeta (caso real de cosmos-laundromat, ver doc de la
            // función) — para TODOS los archivos, no solo el reproducible
            // (el caso real que motivó esto: los archivos de metadata
            // también necesitan estar en su lugar exacto para sembrar 100%).
            let files = resolve_torrent_files(&bytes).unwrap();
            assert_eq!(files.len(), 2);
            assert_eq!(files[0].local_path, std::path::PathBuf::from("mi_item").join("mi_item_meta.xml"));
            assert_eq!(files[0].archive_org_filename, "mi_item_meta.xml");
            assert_eq!(files[1].local_path, std::path::PathBuf::from("mi_item").join("pelicula.mp4"));
            assert_eq!(files[1].archive_org_filename, "pelicula.mp4");
        }
    }
}
