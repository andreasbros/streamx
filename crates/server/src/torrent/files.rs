//! Canonical file listing for a torrent: alphabetical by path,
//! sequentially indexed. Single source of truth for `file_index`
//! semantics across the HTTP API and the in-process API.

use super::engine::TorrentEngine;
use super::types::TorrentFile;
use crate::db::downloads::Download;

/// One file within a torrent, addressable by a stable alphabetical
/// sequential index (`seq_index`). When the torrent is loaded in the
/// engine, `native_index` is set to the per-torrent metadata index
/// needed by `librqbit::api_stream`. When resolved via manifest or
/// disk scan only, `native_index` may be `None`.
pub struct SortedFile {
    pub seq_index: usize,
    pub native_index: Option<usize>,
    pub path: String,
    pub size: u64,
    pub is_video: bool,
    pub is_audio: bool,
}

/// Session first, then the persisted manifest, then a disk scan.
pub async fn sorted_torrent_files(
    engine: &TorrentEngine,
    info_hash: &str,
    download: Option<&Download>,
) -> Vec<SortedFile> {
    // Try active torrent first.
    let _ = engine.ensure_active(info_hash).await;
    let active = engine
        .list_torrent_files(info_hash)
        .await
        .unwrap_or_default();

    if !active.is_empty() {
        let mut files = active;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        return files
            .into_iter()
            .enumerate()
            .map(|(seq, f)| SortedFile {
                seq_index: seq,
                native_index: Some(f.index),
                path: f.path,
                size: f.size,
                is_video: f.is_video,
                is_audio: f.is_audio,
            })
            .collect();
    }

    // Next: the persisted manifest. Stable across restarts and across
    // files moving between partial/ and complete/, so the seq_index the
    // UI cached always maps to the same file even before a re-added
    // torrent's metadata is ready.
    if let Some(manifest) = download.and_then(|d| d.manifest()) {
        if !manifest.is_empty() {
            return manifest
                .into_iter()
                .map(|m| SortedFile {
                    seq_index: m.seq_index,
                    native_index: Some(m.native_index),
                    path: m.path,
                    size: m.size,
                    is_video: m.is_video,
                    is_audio: m.is_audio,
                })
                .collect();
        }
    }

    // Last resort: scan disk (legacy downloads with no manifest). Union
    // both complete/ and partial/ so a download split across the two
    // directories still yields the full, stably-ordered file set. Only
    // safe with a real title — an empty title would scan every download.
    let dl = match download {
        Some(d) if !d.title.trim().is_empty() => d,
        _ => return Vec::new(),
    };

    let partial = engine.partial_dir();
    let complete = engine.complete_dir();
    let mut by_path: std::collections::BTreeMap<String, TorrentFile> =
        std::collections::BTreeMap::new();
    for base in [complete, partial] {
        let dir = base.join(&dl.title);
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.file_name().to_string_lossy().to_string();
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_file() {
                        by_path.entry(path.clone()).or_insert_with(|| TorrentFile {
                            index: 0,
                            path: path.clone(),
                            size: meta.len(),
                            is_video: TorrentFile::detect_video(&path),
                            is_audio: TorrentFile::detect_audio(&path),
                        });
                    }
                }
            }
        } else {
            // Flat single-file torrents put the file directly at the
            // base dir root: named by the row's file_name, or by the
            // torrent title when the two coincide.
            let mut flat_names: Vec<&str> = Vec::new();
            if !dl.file_name.trim().is_empty() {
                flat_names.push(dl.file_name.as_str());
            }
            flat_names.push(dl.title.as_str());
            for name in flat_names {
                if let Ok(meta) = tokio::fs::metadata(base.join(name)).await {
                    if meta.is_file() {
                        let path = name.to_string();
                        by_path.entry(path.clone()).or_insert_with(|| TorrentFile {
                            index: 0,
                            path: path.clone(),
                            size: meta.len(),
                            is_video: TorrentFile::detect_video(&path),
                            is_audio: TorrentFile::detect_audio(&path),
                        });
                        break;
                    }
                }
            }
        }
    }
    by_path
        .into_values()
        .enumerate()
        .map(|(seq, f)| SortedFile {
            seq_index: seq,
            native_index: None,
            path: f.path,
            size: f.size,
            is_video: f.is_video,
            is_audio: f.is_audio,
        })
        .collect()
}
