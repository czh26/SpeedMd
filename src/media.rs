//! 音视频媒体卡片与文件头时长读取（mp4/mov/m4a/wav/flac）。

use crate::html_inline::MediaKind;
use crate::markdown::{bold_family, Theme};
use crate::winutil;
use eframe::egui::{self, CursorIcon, FontId, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Debug, PartialEq)]
pub struct MediaCard {
    pub kind: MediaKind,
    pub url: String,
    pub title: String,
    pub poster: Option<String>,
}

static DURATION_CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<f64>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn format_duration(secs: f64) -> String {
    let total = secs.round().max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

pub fn cached_duration(path: &Path) -> Option<f64> {
    if let Ok(guard) = DURATION_CACHE.lock()
        && let Some(cached) = guard.get(path)
    {
        return *cached;
    }
    let val = read_duration_from_file(path);
    if let Ok(mut guard) = DURATION_CACHE.lock() {
        guard.insert(path.to_path_buf(), val);
    }
    val
}

pub fn read_duration_from_file(path: &Path) -> Option<f64> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    match ext.as_str() {
        "mp4" | "mov" | "m4a" => read_mp4_duration(&mut file, 0, file_len),
        "wav" => read_wav_duration(&mut file, file_len),
        "flac" => read_flac_duration(&mut file),
        _ => None,
    }
}

fn read_mp4_duration<R: Read + Seek>(r: &mut R, start: u64, end: u64) -> Option<f64> {
    let mut pos = start;
    while pos + 8 <= end {
        r.seek(SeekFrom::Start(pos)).ok()?;
        let mut hdr = [0u8; 8];
        r.read_exact(&mut hdr).ok()?;
        let raw_size = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let box_type = [hdr[4], hdr[5], hdr[6], hdr[7]];

        let (box_size, header_len) = if raw_size == 1 {
            if pos + 16 > end {
                break;
            }
            let mut ext = [0u8; 8];
            r.read_exact(&mut ext).ok()?;
            (u64::from_be_bytes(ext), 16u64)
        } else if raw_size == 0 {
            (end - pos, 8u64)
        } else {
            (raw_size, 8u64)
        };
        if box_size < header_len || pos + box_size > end {
            break;
        }
        let payload_start = pos + header_len;
        let payload_end = pos + box_size;

        if &box_type == b"moov" {
            if let Some(d) = read_mp4_duration(r, payload_start, payload_end) {
                return Some(d);
            }
        } else if &box_type == b"mvhd" {
            let need = (payload_end - payload_start).min(64) as usize;
            if need >= 20 {
                let mut buf = vec![0u8; need];
                r.seek(SeekFrom::Start(payload_start)).ok()?;
                r.read_exact(&mut buf).ok()?;
                let version = buf[0];
                if version == 0 && buf.len() >= 20 {
                    let timescale = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]) as f64;
                    let duration = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]) as f64;
                    if timescale > 0.0 {
                        return Some(duration / timescale);
                    }
                } else if version == 1 && buf.len() >= 32 {
                    let timescale = u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]) as f64;
                    let duration = u64::from_be_bytes([
                        buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31],
                    ]) as f64;
                    if timescale > 0.0 {
                        return Some(duration / timescale);
                    }
                }
            }
        }
        pos = payload_end;
    }
    None
}

fn read_wav_duration<R: Read + Seek>(r: &mut R, file_len: u64) -> Option<f64> {
    r.seek(SeekFrom::Start(0)).ok()?;
    let mut riff = [0u8; 12];
    r.read_exact(&mut riff).ok()?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12u64;
    let mut byte_rate: Option<u32> = None;
    let mut data_size: Option<u32> = None;

    while pos + 8 <= file_len {
        r.seek(SeekFrom::Start(pos)).ok()?;
        let mut ch = [0u8; 8];
        r.read_exact(&mut ch).ok()?;
        let id = [ch[0], ch[1], ch[2], ch[3]];
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]);
        let payload = pos + 8;

        if &id == b"fmt " && size >= 12 {
            let mut fmt = [0u8; 12];
            r.read_exact(&mut fmt).ok()?;
            byte_rate = Some(u32::from_le_bytes([fmt[8], fmt[9], fmt[10], fmt[11]]));
        } else if &id == b"data" {
            data_size = Some(size);
        }

        if let (Some(br), Some(ds)) = (byte_rate, data_size)
            && br > 0
        {
            return Some(ds as f64 / br as f64);
        }
        let padded = (size as u64 + 1) & !1;
        pos = payload.saturating_add(padded);
    }
    None
}

fn read_flac_duration<R: Read + Seek>(r: &mut R) -> Option<f64> {
    r.seek(SeekFrom::Start(0)).ok()?;
    let mut sig = [0u8; 4];
    r.read_exact(&mut sig).ok()?;
    if &sig != b"fLaC" {
        return None;
    }
    let mut hdr = [0u8; 4];
    r.read_exact(&mut hdr).ok()?;
    let block_type = hdr[0] & 0x7F;
    let len = u32::from_be_bytes([0, hdr[1], hdr[2], hdr[3]]) as usize;
    if block_type != 0 || len < 18 {
        return None;
    }
    let mut info = [0u8; 18];
    r.read_exact(&mut info).ok()?;
    let val = u64::from_be_bytes([
        info[10], info[11], info[12], info[13], info[14], info[15], info[16], info[17],
    ]);
    let sample_rate = ((val >> 44) & 0xFFFFF) as f64;
    let total_samples = (val & 0xFFFFFFFFF) as f64;
    if sample_rate > 0.0 && total_samples > 0.0 {
        Some(total_samples / sample_rate)
    } else {
        None
    }
}

pub fn render_card_opts(
    ui: &mut egui::Ui,
    card: &MediaCard,
    th: &Theme,
    base_dir: Option<&Path>,
    allow_local_open: bool,
) {
    let is_remote = card.url.starts_with("http://") || card.url.starts_with("https://") || card.url.starts_with("file://");
    let can_open = is_remote || allow_local_open;
    let resolved_path = (!is_remote).then(|| {
        let decoded = crate::markdown::percent_decode(&card.url);
        let p = Path::new(&decoded);
        match base_dir {
            Some(b) if p.is_relative() => b.join(p),
            _ => p.to_path_buf(),
        }
    });
    let duration = resolved_path.as_deref().and_then(cached_duration);

    let icon = match card.kind {
        MediaKind::Video => "🎬",
        MediaKind::Audio => "🎵",
    };
    let kind_label = match card.kind {
        MediaKind::Video => "视频",
        MediaKind::Audio => "音频",
    };
    let display_title = if !card.title.is_empty() {
        card.title.clone()
    } else {
        Path::new(&card.url)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| card.url.clone())
    };

    let resp = egui::Frame::new()
        .fill(th.panel_bg)
        .stroke(Stroke::new(1.0, th.border))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(poster) = &card.poster
                && !poster.is_empty()
            {
                let uri = if poster.starts_with("bytes://") {
                    if poster.starts_with("bytes://thundermd/") {
                        crate::manual::ensure_demo_images(ui.ctx());
                    }
                    poster.clone()
                } else if poster.starts_with("http://") || poster.starts_with("https://") || poster.starts_with("file://") {
                    poster.clone()
                } else {
                    let dec = crate::markdown::percent_decode(poster);
                    let p = match base_dir {
                        Some(b) if Path::new(&dec).is_relative() => b.join(&dec),
                        _ => PathBuf::from(&dec),
                    };
                    format!("file://{}", p.to_string_lossy().replace('\\', "/"))
                };
                ui.add(
                    egui::Image::new(uri)
                        .fit_to_original_size(1.0)
                        .max_width(ui.available_width())
                        .max_height(180.0)
                        .corner_radius(4),
                );
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(th.size * 1.4));
                ui.add_space(6.0);
                let info_w = (ui.available_width() - 68.0).max(80.0);
                ui.allocate_ui_with_layout(egui::vec2(info_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(&display_title)
                                .font(FontId::new(th.size, bold_family()))
                                .color(th.heading),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("[{kind_label}]")).size(th.size * 0.8).color(th.accent));
                        if let Some(secs) = duration {
                            ui.label(RichText::new(format!("  ⏱ {}", format_duration(secs))).size(th.size * 0.85).color(th.weak));
                        }
                    });
                    ui.add(egui::Label::new(RichText::new(&card.url).size(th.size * 0.8).color(th.weak)).truncate());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let action_text = if can_open { "▶ 打开" } else { "演示卡片" };
                    let action_col = if can_open { th.link } else { th.weak };
                    ui.label(RichText::new(action_text).size(th.size * 0.9).color(action_col));
                });
            });
        })
        .response
        .interact(Sense::click());

    if resp.hovered() && can_open {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let clicked = resp.clicked();
    let tip = if can_open {
        format!("使用系统默认程序打开：{}", card.url)
    } else {
        format!("本地媒体路径示例：{}（请替换成你自己的文件路径；说明书中不打开本地文件）", card.url)
    };
    resp.on_hover_text(tip);
    if clicked && can_open {
        let target = match resolved_path {
            Some(p) => p.to_string_lossy().into_owned(),
            None => card.url.clone(),
        };
        let _ = winutil::open_default(&target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_synthetic_mp4_mvhd_duration() {
        // 构造 moov -> mvhd (version 0): timescale = 1000, duration = 65500 -> 65.5s
        let mut mvhd = vec![0u8; 8 + 24];
        let mvhd_len = mvhd.len() as u32;
        mvhd[0..4].copy_from_slice(&mvhd_len.to_be_bytes());
        mvhd[4..8].copy_from_slice(b"mvhd");
        mvhd[8] = 0; // version 0
        mvhd[20..24].copy_from_slice(&1000u32.to_be_bytes());
        mvhd[24..28].copy_from_slice(&65500u32.to_be_bytes());

        let mut moov = vec![0u8; 8];
        moov.extend_from_slice(&mvhd);
        let moov_len = moov.len() as u32;
        moov[0..4].copy_from_slice(&moov_len.to_be_bytes());
        moov[4..8].copy_from_slice(b"moov");

        let mut cur = Cursor::new(&moov);
        let d = read_mp4_duration(&mut cur, 0, moov.len() as u64).unwrap();
        assert!((d - 65.5).abs() < 1e-6);
        assert_eq!(format_duration(d), "01:06");
    }

    #[test]
    fn parse_synthetic_wav_duration() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&0u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        let mut fmt = vec![0u8; 16];
        fmt[8..12].copy_from_slice(&176400u32.to_le_bytes()); // 44100 * 2ch * 16bit
        wav.extend_from_slice(&fmt);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(176400u32 * 5).to_le_bytes());
        let len = wav.len() as u64;
        let mut cur = Cursor::new(&wav);
        let d = read_wav_duration(&mut cur, len).unwrap();
        assert!((d - 5.0).abs() < 1e-6);
    }

    #[test]
    fn parse_synthetic_flac_duration() {
        let mut flac = Vec::new();
        flac.extend_from_slice(b"fLaC");
        flac.push(0x80); // last metadata block, type 0 STREAMINFO
        flac.extend_from_slice(&[0, 0, 34]);
        let mut info = vec![0u8; 34];
        // sample_rate = 44100 (20 bit), total_samples = 44100 * 12 (36 bit)
        let sr = 44100u64;
        let samples = 44100u64 * 12;
        let packed = (sr << 44) | samples;
        info[10..18].copy_from_slice(&packed.to_be_bytes());
        flac.extend_from_slice(&info);

        let mut cur = Cursor::new(&flac);
        let d = read_flac_duration(&mut cur).unwrap();
        assert!((d - 12.0).abs() < 1e-6);
    }
}
