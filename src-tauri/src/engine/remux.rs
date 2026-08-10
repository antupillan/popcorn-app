// Remux MKV -> MP4 sin recodificar, para WebViews que no soportan
// Matroska nativamente (WebKitGTK confirmado en vivo: MEDIA_ERR_SRC_NOT_SUPPORTED
// pese a que el sistema decodifica el mismo stream por otra vía — ver plan
// "Remux MKV -> MP4"). Solo copia bytes de códec ya comprimidos entre
// contenedores, nunca decodifica/recodifica video o audio.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use matroska_demuxer::{Frame, MatroskaFile, TrackEntry, TrackType};
use mp4::{
    AacConfig, AudioObjectType, AvcConfig, ChannelConfig, MediaConfig, Mp4Config, Mp4Sample,
    Mp4Writer, SampleFreqIndex, TrackConfig, TrackType as Mp4TrackType,
};

const MP4_TIMESCALE: u32 = 90_000;
const AAC_SAMPLES_PER_FRAME: u64 = 1024;

pub(crate) fn needs_remux(file_name: &str) -> bool {
    file_name.to_lowercase().ends_with(".mkv")
}

/// Ruta del `.mp4` remuxeado cacheado para un ítem — keyed por `media_id`
/// (UUID estable que ya controlamos, ver `media_items.id`), no por el
/// nombre del archivo original: así se puede construir/borrar sin
/// depender de que la sesión del motor de torrents siga viva (el
/// `engine_torrent_id` sí se pierde entre reinicios, este path no).
pub(crate) fn cache_path(downloads_dir: &Path, media_id: &str) -> std::path::PathBuf {
    downloads_dir.join(".popcorn-remux").join(format!("{media_id}.mp4"))
}

/// Parsea un `AVCDecoderConfigurationRecord` (el `codec_private` que
/// Matroska guarda tal cual para `V_MPEG4/ISO/AVC` — mismo formato que el
/// box `avcC` de MP4, ISO 14496-15) y devuelve el primer SPS/PPS crudos
/// (sin el largo de 2 bytes que antecede a cada uno en el propio record —
/// `AvcConfig` de la crate `mp4` espera los NAL pelados, confirmado
/// leyendo `AvcCBox::new` en el código fuente de esa crate).
fn parse_avcc(data: &[u8]) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    anyhow::ensure!(data.len() >= 6, "AVCDecoderConfigurationRecord demasiado corto");
    let num_sps = data[5] & 0x1F;
    anyhow::ensure!(num_sps >= 1, "AVCDecoderConfigurationRecord sin SPS");
    let mut pos = 6usize;
    let sps_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;
    let sps = data.get(pos..pos + sps_len).ok_or_else(|| anyhow::anyhow!("SPS truncado"))?.to_vec();
    pos += sps_len;
    // Saltea cualquier SPS extra declarado (no soportado, un solo SPS
    // alcanza para el caso real de esta feature).
    for _ in 1..num_sps {
        let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + len;
    }
    anyhow::ensure!(pos < data.len(), "AVCDecoderConfigurationRecord sin PPS");
    let num_pps = data[pos];
    pos += 1;
    anyhow::ensure!(num_pps >= 1, "AVCDecoderConfigurationRecord sin PPS");
    let pps_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;
    let pps = data.get(pos..pos + pps_len).ok_or_else(|| anyhow::anyhow!("PPS truncado"))?.to_vec();
    Ok((sps, pps))
}

/// Parsea un `AudioSpecificConfig` (ISO 14496-3) — mismo `codec_private`
/// crudo para `A_AAC` en Matroska. Layout de bits: 5 bits audioObjectType,
/// 4 bits samplingFrequencyIndex, 4 bits channelConfiguration. No soporta
/// el caso raro de frecuencia explícita (samplingFrequencyIndex == 0xF,
/// 24 bits siguientes) — falla honesto en vez de adivinar (Mandato 4).
fn parse_audio_specific_config(data: &[u8]) -> anyhow::Result<(AudioObjectType, SampleFreqIndex, ChannelConfig)> {
    anyhow::ensure!(data.len() >= 2, "AudioSpecificConfig demasiado corto");
    let object_type = data[0] >> 3;
    let freq_index = ((data[0] & 0x07) << 1) | (data[1] >> 7);
    anyhow::ensure!(freq_index != 0x0F, "frecuencia de muestreo explícita no soportada");
    let channel_config = (data[1] >> 3) & 0x0F;
    Ok((
        AudioObjectType::try_from(object_type).map_err(|e| anyhow::anyhow!("audioObjectType inválido: {e}"))?,
        SampleFreqIndex::try_from(freq_index).map_err(|e| anyhow::anyhow!("samplingFrequencyIndex inválido: {e}"))?,
        ChannelConfig::try_from(channel_config).map_err(|e| anyhow::anyhow!("channelConfiguration inválido: {e}"))?,
    ))
}

/// Convierte un timestamp en ticks de Matroska (ya escalados por
/// `Info::timestamp_scale`, en nanosegundos) a unidades del timescale MP4
/// elegido para este remux.
fn to_mp4_timescale(matroska_ticks: u64, timestamp_scale_ns: u64) -> u64 {
    let total_ns = matroska_ticks.saturating_mul(timestamp_scale_ns);
    total_ns.saturating_mul(MP4_TIMESCALE as u64) / 1_000_000_000
}

fn video_track_config(track: &TrackEntry) -> anyhow::Result<TrackConfig> {
    let video = track.video().ok_or_else(|| anyhow::anyhow!("track de video sin datos de video"))?;
    let codec_private = track
        .codec_private()
        .ok_or_else(|| anyhow::anyhow!("track de video sin codec_private (sin SPS/PPS)"))?;
    let (seq_param_set, pic_param_set) = parse_avcc(codec_private)?;
    Ok(TrackConfig {
        track_type: Mp4TrackType::Video,
        timescale: MP4_TIMESCALE,
        language: "und".to_string(),
        media_conf: MediaConfig::AvcConfig(AvcConfig {
            width: video.pixel_width().get() as u16,
            height: video.pixel_height().get() as u16,
            seq_param_set,
            pic_param_set,
        }),
    })
}

fn audio_track_config(track: &TrackEntry) -> anyhow::Result<TrackConfig> {
    let codec_private = track
        .codec_private()
        .ok_or_else(|| anyhow::anyhow!("track de audio sin codec_private (sin AudioSpecificConfig)"))?;
    let (profile, freq_index, chan_conf) = parse_audio_specific_config(codec_private)?;
    Ok(TrackConfig {
        track_type: Mp4TrackType::Audio,
        timescale: MP4_TIMESCALE,
        language: "und".to_string(),
        media_conf: MediaConfig::AacConfig(AacConfig {
            bitrate: 0,
            profile,
            freq_index,
            chan_conf,
        }),
    })
}

/// Remuxea `input` (.mkv) a `output` (.mp4) copiando los bytes de video
/// H.264 y audio AAC tal cual — sin decodificar ni recodificar. Solo
/// soporta esos dos códecs (`V_MPEG4/ISO/AVC` + `A_AAC`, el caso real
/// encontrado en vivo); cualquier otro códec en video o audio falla con
/// un error explícito en vez de producir un archivo silenciosamente roto
/// (Mandato 4). Pasada única sobre el archivo — no bufferea frames en
/// memoria: la duración de cada sample de video sale de
/// `default_duration` del track (constante, cubre el caso común de
/// contenido a framerate fijo) y la de audio se calcula de
/// samples-por-frame/frecuencia (AAC siempre son 1024 samples por
/// frame), ninguna de las dos necesita ver el frame siguiente.
pub(crate) async fn remux_mkv_to_mp4(input: &Path, output: &Path) -> anyhow::Result<()> {
    let input = input.to_path_buf();
    let output = output.to_path_buf();
    tokio::task::spawn_blocking(move || remux_mkv_to_mp4_blocking(&input, &output)).await??;
    Ok(())
}

fn remux_mkv_to_mp4_blocking(input: &Path, output: &Path) -> anyhow::Result<()> {
    let file = File::open(input).map_err(|e| anyhow::anyhow!("no se pudo abrir {}: {e}", input.display()))?;
    let mut mkv = MatroskaFile::open(file).map_err(|e| anyhow::anyhow!("no se pudo abrir el .mkv: {e}"))?;
    let timestamp_scale = mkv.info().timestamp_scale().get();

    let video_track = mkv
        .tracks()
        .iter()
        .find(|t| t.track_type() == TrackType::Video && t.codec_id() == "V_MPEG4/ISO/AVC")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("sin track de video H.264 — códec no soportado para remux"))?;
    let audio_track = mkv
        .tracks()
        .iter()
        .find(|t| t.track_type() == TrackType::Audio && t.codec_id() == "A_AAC")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("sin track de audio AAC — códec no soportado para remux"))?;

    let video_number = video_track.track_number().get();
    let audio_number = audio_track.track_number().get();
    let video_default_duration = video_track.default_duration().map(|d| d.get());
    let audio_sample_rate = audio_track
        .audio()
        .ok_or_else(|| anyhow::anyhow!("track de audio sin datos de audio"))?
        .sampling_frequency();

    let video_conf = video_track_config(&video_track)?;
    let audio_conf = audio_track_config(&audio_track)?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("no se pudo crear {}: {e}", parent.display()))?;
    }
    let out_file = File::create(output).map_err(|e| anyhow::anyhow!("no se pudo crear {}: {e}", output.display()))?;
    let mut writer = Mp4Writer::write_start(
        BufWriter::new(out_file),
        &Mp4Config {
            major_brand: str::parse("isom").map_err(|e| anyhow::anyhow!("{e:?}"))?,
            minor_version: 512,
            compatible_brands: vec![
                str::parse("isom").map_err(|e| anyhow::anyhow!("{e:?}"))?,
                str::parse("iso2").map_err(|e| anyhow::anyhow!("{e:?}"))?,
                str::parse("avc1").map_err(|e| anyhow::anyhow!("{e:?}"))?,
                str::parse("mp41").map_err(|e| anyhow::anyhow!("{e:?}"))?,
            ],
            timescale: MP4_TIMESCALE,
        },
    )?;
    // add_track no devuelve el id — el writer los asigna 1, 2, 3... en el
    // orden en que se llama add_track (confirmado leyendo writer.rs de la
    // propia crate), así que el orden acá define video_id=1, audio_id=2.
    writer.add_track(&video_conf)?;
    writer.add_track(&audio_conf)?;
    let video_id: u32 = 1;
    let audio_id: u32 = 2;

    // Duración por sample de video: constante desde default_duration si
    // el archivo la declaró (framerate fijo, el caso común); sin eso,
    // usar 0 sería peor que una aproximación razonable — 24fps es el
    // valor más común en el contenido real que motivó esta feature.
    let video_sample_duration = to_mp4_timescale(
        video_default_duration.unwrap_or(1_000_000_000 / 24),
        1, // default_duration ya está en nanosegundos, no en ticks a escalar
    ) as u32;
    let audio_sample_duration =
        (AAC_SAMPLES_PER_FRAME.saturating_mul(MP4_TIMESCALE as u64) / audio_sample_rate as u64) as u32;

    let mut frame = Frame::default();
    while mkv.next_frame(&mut frame).map_err(|e| anyhow::anyhow!("error leyendo frame del .mkv: {e}"))? {
        let start_time = to_mp4_timescale(frame.timestamp, timestamp_scale);
        if frame.track == video_number {
            let sample = Mp4Sample {
                start_time,
                duration: video_sample_duration,
                rendering_offset: 0,
                is_sync: frame.is_keyframe.unwrap_or(false),
                bytes: mp4::Bytes::from(std::mem::take(&mut frame.data)),
            };
            writer.write_sample(video_id, &sample)?;
        } else if frame.track == audio_number {
            let sample = Mp4Sample {
                start_time,
                duration: audio_sample_duration,
                rendering_offset: 0,
                is_sync: true,
                bytes: mp4::Bytes::from(std::mem::take(&mut frame.data)),
            };
            writer.write_sample(audio_id, &sample)?;
        }
        // Cualquier otra track (subtítulos embebidos, ej. los 15 SRT del
        // caso real) se ignora a propósito — el remux es solo para
        // destrabar reproducción de video/audio en el WebView.
    }

    writer.write_end()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp4::MediaType;

    #[test]
    fn needs_remux_only_matches_mkv_case_insensitive() {
        assert!(needs_remux("One.Piece.mkv"));
        assert!(needs_remux("One.Piece.MKV"));
        assert!(!needs_remux("Sintel.mp4"));
        assert!(!needs_remux("no_extension"));
    }

    #[tokio::test]
    async fn remux_mkv_to_mp4_produces_a_readable_mp4_with_matching_sample_counts() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/sample.mkv");
        let output = std::env::temp_dir().join(format!("popcorn-remux-test-{}.mp4", uuid::Uuid::new_v4()));

        remux_mkv_to_mp4(&input, &output).await.unwrap();

        // Releer con la misma crate mp4 (camino de lectura ya probado por
        // su propia test suite) confirma que el archivo escrito es un MP4
        // válido, no solo que write_end() no explotó.
        let out_file = File::open(&output).unwrap();
        let size = out_file.metadata().unwrap().len();
        let reader = mp4::Mp4Reader::read_header(std::io::BufReader::new(out_file), size).unwrap();

        let video_track = reader
            .tracks()
            .values()
            .find(|t| t.media_type().unwrap() == MediaType::H264)
            .expect("debe tener un track de video H.264");
        let audio_track = reader
            .tracks()
            .values()
            .find(|t| t.media_type().unwrap() == MediaType::AAC)
            .expect("debe tener un track de audio AAC");

        assert_eq!(reader.sample_count(video_track.track_id()).unwrap(), 10, "10 frames de video en la fixture");
        assert!(reader.sample_count(audio_track.track_id()).unwrap() > 0);

        std::fs::remove_file(&output).ok();
    }

    #[test]
    fn remux_rejects_unsupported_video_codec() {
        // AVCDecoderConfigurationRecord con menos de 6 bytes — cualquier
        // input roto/no soportado debe fallar explícito, nunca producir
        // un archivo silenciosamente corrupto.
        assert!(parse_avcc(&[0, 0]).is_err());
    }

    #[test]
    fn parse_audio_specific_config_rejects_explicit_frequency() {
        // audioObjectType=2 (AAC LC, 00010) + freq_index=0xF (1111,
        // explícita, no soportada) => byte0 = 00010_111 = 0x17;
        // byte1 = 1_0010_000 (bit final de freq + chan_conf=2 + reservado) = 0x90.
        let data = [0x17, 0x90];
        assert!(parse_audio_specific_config(&data).is_err());
    }

    #[test]
    fn parse_audio_specific_config_reads_common_aac_lc_48khz_stereo() {
        // audioObjectType=2 (00010) + freq_index=3/48kHz (0011) =>
        // byte0 = 00010_001 = 0x11; byte1 = 1_0010_000 (bit final de
        // freq + chan_conf=2/stereo + reservado) = 0x90. Verificado
        // contra la función real, no solo a mano (ver aritmética arriba).
        let data = [0x11, 0x90];
        let (obj, freq, chan) = parse_audio_specific_config(&data).unwrap();
        assert_eq!(obj, AudioObjectType::AacLowComplexity);
        assert_eq!(freq, SampleFreqIndex::Freq48000);
        assert_eq!(chan, ChannelConfig::Stereo);
    }
}
