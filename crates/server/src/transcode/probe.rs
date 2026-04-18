use crate::error::{Error, Result};
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub duration_seconds: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub container: Option<String>,
    pub bit_depth: Option<u32>,
    pub color_space: Option<String>,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub hdr_format: HdrFormat,
    pub audio_channels: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub audio_format_name: Option<String>,
    pub has_dolby_vision: bool,
    pub has_dolby_atmos: bool,
    pub has_dts: bool,
    pub has_hdr10: bool,
    pub has_hdr10_plus: bool,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub needs_transcode: bool,
    pub needs_audio_transcode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HdrFormat {
    Hdr10,
    Hdr10Plus,
    DolbyVision,
    Hlg,
    None,
}

#[derive(Debug, Clone)]
pub struct SubtitleTrack {
    pub index: usize,
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: String,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    format: Option<FfprobeFormat>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct FfprobeStream {
    #[serde(default)]
    codec_type: Option<String>,
    #[serde(default)]
    codec_name: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    pix_fmt: Option<String>,
    #[serde(default)]
    color_space: Option<String>,
    #[serde(default)]
    color_transfer: Option<String>,
    #[serde(default)]
    color_primaries: Option<String>,
    #[serde(default)]
    bits_per_raw_sample: Option<String>,
    #[serde(default)]
    channels: Option<u32>,
    #[serde(default)]
    sample_rate: Option<String>,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    side_data_list: Option<Vec<SideData>>,
    #[serde(default)]
    tags: Option<StreamTags>,
}

#[derive(Debug, Deserialize)]
struct SideData {
    #[serde(default)]
    side_data_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamTags {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    format_name: Option<String>,
}

pub async fn probe(file_path: &str) -> Result<MediaInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            file_path,
        ])
        .output()
        .await
        .map_err(|e| Error::Transcode {
            message: format!("Failed to run ffprobe: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(Error::Transcode {
            message: format!(
                "ffprobe failed (exit {}): stderr={stderr} stdout={stdout} file={file_path}",
                output.status.code().unwrap_or(-1)
            ),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let probe_data: FfprobeOutput =
        serde_json::from_str(&stdout).map_err(|e| Error::Transcode {
            message: format!("Failed to parse ffprobe output: {e}"),
        })?;

    parse_probe_output(probe_data)
}

fn parse_probe_output(data: FfprobeOutput) -> Result<MediaInfo> {
    let video_stream = data
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"));

    let audio_stream = data
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"));

    let video_codec = video_stream.and_then(|s| s.codec_name.clone());
    let audio_codec = audio_stream.and_then(|s| s.codec_name.clone());

    let width = video_stream.and_then(|s| s.width);
    let height = video_stream.and_then(|s| s.height);

    let bit_depth = video_stream
        .and_then(|s| s.bits_per_raw_sample.as_deref())
        .and_then(|b| b.parse::<u32>().ok());

    let color_space = video_stream.and_then(|s| s.color_space.clone());
    let color_transfer = video_stream.and_then(|s| s.color_transfer.clone());
    let color_primaries = video_stream.and_then(|s| s.color_primaries.clone());

    let has_dolby_vision = video_stream
        .and_then(|s| s.side_data_list.as_ref())
        .map(|side_data| {
            side_data.iter().any(|sd| {
                sd.side_data_type
                    .as_deref()
                    .map(|t| t.contains("DOVI") || t.contains("Dolby Vision"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let has_hdr10 = color_transfer.as_deref() == Some("smpte2084") && !has_dolby_vision;

    let has_hdr10_plus = video_stream
        .and_then(|s| s.side_data_list.as_ref())
        .map(|side_data| {
            side_data.iter().any(|sd| {
                sd.side_data_type
                    .as_deref()
                    .map(|t| t.contains("HDR10+") || t.contains("HDR Dynamic Metadata"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let is_hlg = color_transfer.as_deref() == Some("arib-std-b67");

    let hdr_format = if has_dolby_vision {
        HdrFormat::DolbyVision
    } else if has_hdr10_plus {
        HdrFormat::Hdr10Plus
    } else if has_hdr10 {
        HdrFormat::Hdr10
    } else if is_hlg {
        HdrFormat::Hlg
    } else {
        HdrFormat::None
    };

    let audio_channels = audio_stream.and_then(|s| s.channels);
    let audio_sample_rate = audio_stream
        .and_then(|s| s.sample_rate.as_deref())
        .and_then(|r| r.parse::<u32>().ok());
    let audio_format_name = audio_codec.clone();

    let has_dts = audio_codec
        .as_deref()
        .map(|c| c.starts_with("dts"))
        .unwrap_or(false);

    let has_dolby_atmos = audio_codec.as_deref() == Some("truehd")
        && audio_channels.map(|ch| ch >= 8).unwrap_or(false);

    let duration_seconds = data
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok());

    let container = data.format.as_ref().and_then(|f| f.format_name.clone());

    let subtitle_tracks: Vec<SubtitleTrack> = data
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("subtitle"))
        .map(|s| SubtitleTrack {
            index: s.index.unwrap_or(0),
            language: s.tags.as_ref().and_then(|t| t.language.clone()),
            title: s.tags.as_ref().and_then(|t| t.title.clone()),
            codec: s.codec_name.clone().unwrap_or_default(),
        })
        .collect();

    let needs_video_transcode = match video_codec.as_deref() {
        Some("h264") => hdr_format != HdrFormat::None,
        Some(_) => true,
        None => false,
    };

    let needs_audio_transcode = match audio_codec.as_deref() {
        Some("aac") | Some("mp3") | Some("opus") => false,
        Some(_) => true,
        None => false,
    };

    let needs_transcode = needs_video_transcode || needs_audio_transcode;

    Ok(MediaInfo {
        duration_seconds,
        video_codec,
        audio_codec,
        width,
        height,
        container,
        bit_depth,
        color_space,
        color_transfer,
        color_primaries,
        hdr_format,
        audio_channels,
        audio_sample_rate,
        audio_format_name,
        has_dolby_vision,
        has_dolby_atmos,
        has_dts,
        has_hdr10,
        has_hdr10_plus,
        subtitle_tracks,
        needs_transcode,
        needs_audio_transcode,
    })
}

pub fn is_browser_compatible(info: &MediaInfo) -> bool {
    let compatible_video = info
        .video_codec
        .as_deref()
        .map(|c| c == "h264" || c == "avc1")
        .unwrap_or(false);

    let compatible_audio = info
        .audio_codec
        .as_deref()
        .map(|c| c == "aac" || c == "mp3" || c == "mp4a" || c == "opus")
        .unwrap_or(true);

    let compatible_container = info
        .container
        .as_deref()
        .map(|c| {
            let is_mp4 = c.contains("mp4") || c.contains("mov");
            let is_webm = c == "webm";
            let is_mpegts = c.contains("mpegts");
            is_mp4 || is_webm || is_mpegts
        })
        .unwrap_or(false);

    let no_hdr = info.hdr_format == HdrFormat::None;

    compatible_video && compatible_audio && compatible_container && no_hdr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_compatible_h264_aac_mp4() {
        let info = MediaInfo {
            duration_seconds: Some(120.0),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            width: Some(1920),
            height: Some(1080),
            container: Some("mov,mp4,m4a,3gp,3g2,mj2".to_string()),
            bit_depth: Some(8),
            color_space: None,
            color_transfer: None,
            color_primaries: None,
            hdr_format: HdrFormat::None,
            audio_channels: Some(2),
            audio_sample_rate: Some(48000),
            audio_format_name: Some("aac".to_string()),
            has_dolby_vision: false,
            has_dolby_atmos: false,
            has_dts: false,
            has_hdr10: false,
            has_hdr10_plus: false,
            subtitle_tracks: vec![],
            needs_transcode: false,
            needs_audio_transcode: false,
        };
        assert!(is_browser_compatible(&info));
    }

    #[test]
    fn hevc_needs_transcode() {
        let info = MediaInfo {
            duration_seconds: Some(120.0),
            video_codec: Some("hevc".to_string()),
            audio_codec: Some("aac".to_string()),
            width: Some(3840),
            height: Some(2160),
            container: Some("matroska,webm".to_string()),
            bit_depth: Some(10),
            color_space: Some("bt2020nc".to_string()),
            color_transfer: Some("smpte2084".to_string()),
            color_primaries: Some("bt2020".to_string()),
            hdr_format: HdrFormat::Hdr10,
            audio_channels: Some(2),
            audio_sample_rate: Some(48000),
            audio_format_name: Some("aac".to_string()),
            has_dolby_vision: false,
            has_dolby_atmos: false,
            has_dts: false,
            has_hdr10: true,
            has_hdr10_plus: false,
            subtitle_tracks: vec![],
            needs_transcode: true,
            needs_audio_transcode: false,
        };
        assert!(!is_browser_compatible(&info));
    }

    #[test]
    fn truehd_audio_needs_transcode() {
        let data = FfprobeOutput {
            streams: vec![
                FfprobeStream {
                    codec_type: Some("video".to_string()),
                    codec_name: Some("hevc".to_string()),
                    width: Some(3840),
                    height: Some(2160),
                    pix_fmt: Some("yuv420p10le".to_string()),
                    color_space: None,
                    color_transfer: None,
                    color_primaries: None,
                    bits_per_raw_sample: Some("10".to_string()),
                    channels: None,
                    sample_rate: None,
                    index: Some(0),
                    side_data_list: None,
                    tags: None,
                },
                FfprobeStream {
                    codec_type: Some("audio".to_string()),
                    codec_name: Some("truehd".to_string()),
                    width: None,
                    height: None,
                    pix_fmt: None,
                    color_space: None,
                    color_transfer: None,
                    color_primaries: None,
                    bits_per_raw_sample: None,
                    channels: Some(8),
                    sample_rate: Some("48000".to_string()),
                    index: Some(1),
                    side_data_list: None,
                    tags: None,
                },
            ],
            format: Some(FfprobeFormat {
                duration: Some("7200.0".to_string()),
                format_name: Some("matroska,webm".to_string()),
            }),
        };

        let info = parse_probe_output(data).unwrap();
        assert!(info.needs_transcode);
        assert!(info.needs_audio_transcode);
        assert!(info.has_dolby_atmos);
        assert_eq!(info.audio_channels, Some(8));
    }

    #[test]
    fn dolby_vision_detection() {
        let data = FfprobeOutput {
            streams: vec![FfprobeStream {
                codec_type: Some("video".to_string()),
                codec_name: Some("hevc".to_string()),
                width: Some(3840),
                height: Some(2160),
                pix_fmt: Some("yuv420p10le".to_string()),
                color_space: Some("bt2020nc".to_string()),
                color_transfer: Some("smpte2084".to_string()),
                color_primaries: Some("bt2020".to_string()),
                bits_per_raw_sample: Some("10".to_string()),
                channels: None,
                sample_rate: None,
                index: Some(0),
                side_data_list: Some(vec![SideData {
                    side_data_type: Some("DOVI configuration record".to_string()),
                }]),
                tags: None,
            }],
            format: Some(FfprobeFormat {
                duration: Some("7200.0".to_string()),
                format_name: Some("matroska,webm".to_string()),
            }),
        };

        let info = parse_probe_output(data).unwrap();
        assert!(info.has_dolby_vision);
        assert!(!info.has_hdr10);
        assert_eq!(info.hdr_format, HdrFormat::DolbyVision);
    }

    #[test]
    fn subtitle_tracks_parsed() {
        let data = FfprobeOutput {
            streams: vec![
                FfprobeStream {
                    codec_type: Some("video".to_string()),
                    codec_name: Some("h264".to_string()),
                    width: Some(1920),
                    height: Some(1080),
                    pix_fmt: None,
                    color_space: None,
                    color_transfer: None,
                    color_primaries: None,
                    bits_per_raw_sample: None,
                    channels: None,
                    sample_rate: None,
                    index: Some(0),
                    side_data_list: None,
                    tags: None,
                },
                FfprobeStream {
                    codec_type: Some("subtitle".to_string()),
                    codec_name: Some("subrip".to_string()),
                    width: None,
                    height: None,
                    pix_fmt: None,
                    color_space: None,
                    color_transfer: None,
                    color_primaries: None,
                    bits_per_raw_sample: None,
                    channels: None,
                    sample_rate: None,
                    index: Some(2),
                    side_data_list: None,
                    tags: Some(StreamTags {
                        language: Some("eng".to_string()),
                        title: Some("English".to_string()),
                    }),
                },
            ],
            format: Some(FfprobeFormat {
                duration: Some("120.0".to_string()),
                format_name: Some("matroska,webm".to_string()),
            }),
        };

        let info = parse_probe_output(data).unwrap();
        assert_eq!(info.subtitle_tracks.len(), 1);
        assert_eq!(info.subtitle_tracks[0].codec, "subrip");
        assert_eq!(info.subtitle_tracks[0].language.as_deref(), Some("eng"));
    }
}
