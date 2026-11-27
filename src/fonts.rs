//! 系统字体扫描（按偏移读取表目录与 name / OS/2 表）与按选择加载字体。

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontTweak};
use read_fonts::{
    tables::{
        name::{Name, NameId},
        os2::{Os2, SelectionFlags},
    },
    types::GlyphId,
    FontData as RfFontData, FontRead, FontRef, TableProvider,
};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

#[derive(Clone, Debug)]
pub struct FontFaceLoc {
    pub path: PathBuf,
    pub ttc_index: u32,
}

#[derive(Clone, Debug)]
pub struct FontFamilyInfo {
    pub name: String,
    pub regular: Option<FontFaceLoc>,
    pub bold: Option<FontFaceLoc>,
    pub is_mono: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FontCatalog {
    pub families: BTreeMap<String, FontFamilyInfo>,
}

impl FontCatalog {
    pub fn all_names(&self) -> Vec<String> {
        self.families.keys().cloned().collect()
    }

    pub fn mono_names(&self) -> Vec<String> {
        self.families.values().filter(|f| f.is_mono).map(|f| f.name.clone()).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FontChoice {
    pub preview: String,
    pub editor: String,
    pub code: String,
}

fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    dirs.push(PathBuf::from(windir).join("Fonts"));
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
    }
    dirs
}

fn read_be_u16(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off + 2)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

fn read_be_u32(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

struct ParsedFace {
    family: String,
    weight: u16,
    italic: bool,
    is_mono: bool,
    ttc_index: u32,
}

fn parse_face_at(file: &mut File, face_offset: u64, ttc_index: u32) -> Option<ParsedFace> {
    file.seek(SeekFrom::Start(face_offset)).ok()?;
    let mut sfnt = [0u8; 12];
    file.read_exact(&mut sfnt).ok()?;
    let num_tables = read_be_u16(&sfnt, 4)? as usize;
    if num_tables == 0 || num_tables > 256 {
        return None;
    }

    let mut dir = vec![0u8; num_tables * 16];
    file.read_exact(&mut dir).ok()?;

    let mut name_rec: Option<(u32, u32)> = None;
    let mut os2_rec: Option<(u32, u32)> = None;
    for chunk in dir.as_chunks::<16>().0 {
        let tag = &chunk[0..4];
        let off = read_be_u32(chunk, 8)?;
        let len = read_be_u32(chunk, 12)?;
        if tag == b"name" {
            name_rec = Some((off, len));
        } else if tag == b"OS/2" {
            os2_rec = Some((off, len));
        }
    }

    let (name_off, name_len) = name_rec?;
    if name_len == 0 || name_len > 1024 * 1024 {
        return None;
    }
    let mut name_bytes = vec![0u8; name_len as usize];
    file.seek(SeekFrom::Start(name_off as u64)).ok()?;
    file.read_exact(&mut name_bytes).ok()?;

    let name_table = Name::read(RfFontData::new(&name_bytes)).ok()?;
    let str_data = name_table.string_data();

    let mut typo_cn: Option<String> = None;
    let mut typo_en: Option<String> = None;
    let mut typo_any: Option<String> = None;
    let mut fam_cn: Option<String> = None;
    let mut fam_en: Option<String> = None;
    let mut fam_any: Option<String> = None;

    for rec in name_table.name_record() {
        if !rec.is_unicode() {
            continue;
        }
        let nid = rec.name_id();
        if nid != NameId::TYPOGRAPHIC_FAMILY_NAME && nid != NameId::FAMILY_NAME {
            continue;
        }
        let Ok(ns) = rec.string(str_data) else { continue };
        let val = ns.to_string().trim().to_string();
        if val.is_empty() || val.starts_with('@') {
            continue;
        }
        let lang = rec.language_id();
        if nid == NameId::TYPOGRAPHIC_FAMILY_NAME {
            if lang == 0x0804 && typo_cn.is_none() {
                typo_cn = Some(val);
            } else if lang == 0x0409 && typo_en.is_none() {
                typo_en = Some(val);
            } else if typo_any.is_none() {
                typo_any = Some(val);
            }
        } else if lang == 0x0804 && fam_cn.is_none() {
            fam_cn = Some(val);
        } else if lang == 0x0409 && fam_en.is_none() {
            fam_en = Some(val);
        } else if fam_any.is_none() {
            fam_any = Some(val);
        }
    }

    let family = typo_cn.or(fam_cn).or(typo_en).or(fam_en).or(typo_any).or(fam_any)?;

    let (mut weight, mut italic, mut is_mono) = (400u16, false, false);
    if let Some((os2_off, os2_len)) = os2_rec
        && (68..=512).contains(&os2_len)
    {
        let mut os2_bytes = vec![0u8; os2_len as usize];
        file.seek(SeekFrom::Start(os2_off as u64)).ok()?;
        if file.read_exact(&mut os2_bytes).is_ok()
            && let Ok(os2) = Os2::read(RfFontData::new(&os2_bytes))
        {
            weight = os2.us_weight_class();
            italic = os2.fs_selection().intersects(SelectionFlags::ITALIC | SelectionFlags::OBLIQUE);
            is_mono = os2.panose_10().get(3) == Some(&9);
        }
    }

    Some(ParsedFace { family, weight, italic, is_mono, ttc_index })
}

fn scan_file(path: &Path) -> Vec<ParsedFace> {
    let Ok(mut file) = File::open(path) else { return Vec::new() };
    let mut head = [0u8; 12];
    if file.read_exact(&mut head).is_err() {
        return Vec::new();
    }
    if &head[0..4] == b"ttcf" {
        let Some(num_fonts) = read_be_u32(&head, 8) else { return Vec::new() };
        let count = (num_fonts as usize).min(64);
        let mut offsets = vec![0u8; count * 4];
        if file.read_exact(&mut offsets).is_err() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            if let Some(off) = read_be_u32(&offsets, i * 4)
                && let Some(face) = parse_face_at(&mut file, off as u64, i as u32)
            {
                out.push(face);
            }
        }
        out
    } else if matches!(&head[0..4], [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true") {
        parse_face_at(&mut file, 0, 0).into_iter().collect()
    } else {
        Vec::new()
    }
}

pub fn scan_system_fonts() -> FontCatalog {
    let mut families: BTreeMap<String, FontFamilyInfo> = BTreeMap::new();
    for dir in system_font_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_font = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"));
            if !is_font {
                continue;
            }
            for face in scan_file(&path) {
                if face.italic {
                    continue;
                }
                let loc = FontFaceLoc { path: path.clone(), ttc_index: face.ttc_index };
                let info = families.entry(face.family.clone()).or_insert_with(|| FontFamilyInfo {
                    name: face.family.clone(),
                    regular: None,
                    bold: None,
                    is_mono: false,
                });
                info.is_mono |= face.is_mono;
                if (350..=550).contains(&face.weight) {
                    if info.regular.is_none() || face.weight == 400 {
                        info.regular = Some(loc);
                    }
                } else if face.weight >= 600 && (info.bold.is_none() || face.weight == 700) {
                    info.bold = Some(loc);
                } else if info.regular.is_none() {
                    info.regular = Some(loc);
                }
            }
        }
    }
    FontCatalog { families }
}

pub fn start_scan(ctx: egui::Context) -> mpsc::Receiver<FontCatalog> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let cat = scan_system_fonts();
        let _ = tx.send(cat);
        ctx.request_repaint();
    });
    rx
}

pub fn setup(ctx: &egui::Context) {
    apply_choice(ctx, None, &FontChoice::default());
}

const MAX_SYNC_BLOB_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
struct VerticalMetrics {
    ascent_em: f32,
    descent_em: f32,
    gap_em: f32,
    has_cjk: bool,
}

impl VerticalMetrics {
    /// epaint 将回退字体行高盒居中进主字体行高盒，主/辅字体的基线偏移由此中点因子决定。
    fn mid_factor(self) -> f32 {
        0.5 * (self.ascent_em + self.descent_em - self.gap_em)
    }
}

/// 按 skrifa::metrics::Metrics 的优先级解析字体垂直度量及是否含常用汉字。
fn read_vertical_metrics(bytes: &[u8], ttc_index: u32) -> Option<VerticalMetrics> {
    let font = FontRef::from_index(bytes, ttc_index).ok()?;
    let upem = font.head().ok()?.units_per_em() as f32;
    if upem <= 0.0 {
        return None;
    }
    let os2 = font.os2().ok();
    let mut asc = 0.0_f32;
    let mut desc = 0.0_f32;
    let mut gap = 0.0_f32;
    let mut used_typo = false;
    if let Some(os2) = &os2
        && os2.fs_selection().contains(SelectionFlags::USE_TYPO_METRICS)
    {
        asc = os2.s_typo_ascender() as f32;
        desc = os2.s_typo_descender() as f32;
        gap = os2.s_typo_line_gap() as f32;
        used_typo = true;
    }
    if !used_typo {
        if let Ok(hhea) = font.hhea() {
            asc = hhea.ascender().to_i16() as f32;
            desc = hhea.descender().to_i16() as f32;
            gap = hhea.line_gap().to_i16() as f32;
        }
        if asc == 0.0
            && desc == 0.0
            && let Some(os2) = &os2
        {
            if os2.s_typo_ascender() != 0 || os2.s_typo_descender() != 0 {
                asc = os2.s_typo_ascender() as f32;
                desc = os2.s_typo_descender() as f32;
                gap = os2.s_typo_line_gap() as f32;
            } else {
                asc = os2.us_win_ascent() as f32;
                desc = -(os2.us_win_descent() as f32);
            }
        }
    }
    let has_cjk = font
        .cmap()
        .ok()
        .and_then(|c| c.map_codepoint('中'))
        .is_some_and(|gid| gid != GlyphId::NOTDEF);
    Some(VerticalMetrics {
        ascent_em: asc / upem,
        descent_em: desc / upem,
        gap_em: gap / upem,
        has_cjk,
    })
}

fn write_be_i16(buf: &mut [u8], off: usize, v: i16) {
    buf[off..off + 2].copy_from_slice(&v.to_be_bytes());
}

/// 在内存副本中把非 CJK 主字体的 hhea / OS/2 typo 垂直度量对齐到目标 CJK 度量，
/// 使 epaint 中英混排的基线与 logical_rect（背景块/光标/删除线）同步对齐。
fn sync_vertical_metrics(blob: &[u8], ttc_index: u32, target: VerticalMetrics) -> Option<Vec<u8>> {
    if blob.len() > MAX_SYNC_BLOB_BYTES || blob.len() < 12 {
        return None;
    }
    if FontRef::from_index(blob, ttc_index).ok()?.mvar().is_ok() {
        return None;
    }

    let face_offset = if &blob[0..4] == b"ttcf" {
        let num_fonts = read_be_u32(blob, 8)?;
        if ttc_index >= num_fonts {
            return None;
        }
        read_be_u32(blob, 12 + ttc_index as usize * 4)? as usize
    } else if ttc_index == 0 && matches!(&blob[0..4], [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true") {
        0
    } else {
        return None;
    };

    let num_tables = read_be_u16(blob, face_offset.checked_add(4)?)? as usize;
    if !(1..=256).contains(&num_tables) {
        return None;
    }
    let dir_start = face_offset.checked_add(12)?;
    let dir_end = dir_start.checked_add(num_tables * 16)?;
    let dir = blob.get(dir_start..dir_end)?;

    let mut head_rec: Option<(usize, usize)> = None;
    let mut hhea_rec: Option<(usize, usize)> = None;
    let mut os2_rec: Option<(usize, usize)> = None;
    for chunk in dir.as_chunks::<16>().0 {
        let tag = &chunk[0..4];
        let off = read_be_u32(chunk, 8)? as usize;
        let len = read_be_u32(chunk, 12)? as usize;
        if off.checked_add(len).is_none_or(|end| end > blob.len()) {
            continue;
        }
        match tag {
            b"head" => head_rec = Some((off, len)),
            b"hhea" => hhea_rec = Some((off, len)),
            b"OS/2" => os2_rec = Some((off, len)),
            _ => {}
        }
    }

    let (head_off, head_len) = head_rec?;
    let (hhea_off, hhea_len) = hhea_rec?;
    if head_len < 20 || hhea_len < 10 {
        return None;
    }
    let upem = read_be_u16(blob, head_off + 18)?;
    if upem < 64 {
        return None;
    }

    let new_asc = (target.ascent_em * upem as f32).round().clamp(-32000.0, 32000.0) as i16;
    let new_desc = (target.descent_em * upem as f32).round().clamp(-32000.0, 32000.0) as i16;
    let new_gap = (target.gap_em * upem as f32).round().clamp(0.0, 32000.0) as i16;

    let mut out = blob.to_vec();
    write_be_i16(&mut out, hhea_off + 4, new_asc);
    write_be_i16(&mut out, hhea_off + 6, new_desc);
    write_be_i16(&mut out, hhea_off + 8, new_gap);
    if let Some((os2_off, os2_len)) = os2_rec
        && os2_len >= 74
    {
        write_be_i16(&mut out, os2_off + 68, new_asc);
        write_be_i16(&mut out, os2_off + 70, new_desc);
        write_be_i16(&mut out, os2_off + 72, new_gap);
    }

    FontRef::from_index(&out, ttc_index).is_ok().then_some(out)
}

#[derive(Default)]
struct BlobCache {
    raw: HashMap<PathBuf, Arc<Vec<u8>>>,
    synced: HashMap<(PathBuf, u32), Arc<Vec<u8>>>,
}

impl BlobCache {
    fn read_raw(&mut self, path: &Path) -> Option<Arc<Vec<u8>>> {
        if let Some(b) = self.raw.get(path) {
            return Some(Arc::clone(b));
        }
        let bytes = std::fs::read(path).ok()?;
        let arc = Arc::new(bytes);
        self.raw.insert(path.to_path_buf(), Arc::clone(&arc));
        Some(arc)
    }

    fn resolve_blob(
        &mut self,
        loc: &FontFaceLoc,
        sync_target: Option<VerticalMetrics>,
    ) -> Option<(Arc<Vec<u8>>, Option<VerticalMetrics>)> {
        let raw = self.read_raw(&loc.path)?;
        let orig_m = read_vertical_metrics(&raw, loc.ttc_index);
        let should_sync = sync_target.filter(|cm| {
            orig_m.is_some_and(|om| !om.has_cjk && (om.mid_factor() - cm.mid_factor()).abs() > 0.02)
        });
        if let Some(cm) = should_sync {
            let key = (loc.path.clone(), loc.ttc_index);
            if let Some(cached) = self.synced.get(&key) {
                let eff_m = read_vertical_metrics(cached, loc.ttc_index).or(orig_m);
                return Some((Arc::clone(cached), eff_m));
            }
            if let Some(patched) = sync_vertical_metrics(&raw, loc.ttc_index, cm) {
                let arc = Arc::new(patched);
                let eff_m = read_vertical_metrics(&arc, loc.ttc_index).or(orig_m);
                self.synced.insert(key, Arc::clone(&arc));
                return Some((arc, eff_m));
            }
        }
        Some((raw, orig_m))
    }
}

fn register_font(
    fonts: &mut FontDefinitions,
    name: &str,
    blob: Arc<Vec<u8>>,
    ttc_index: u32,
    tweak: Option<&FontTweak>,
) -> String {
    let mut fd = FontData::from_blob(blob, ttc_index);
    if let Some(tw) = tweak {
        fd = fd.tweak(tw.clone());
    }
    fonts.font_data.insert(name.into(), Arc::new(fd));
    name.to_string()
}

fn load_default_loc(
    caches: &mut BlobCache,
    dir: &Path,
    files: &[&str],
) -> Option<(FontFaceLoc, Arc<Vec<u8>>, Option<VerticalMetrics>)> {
    for f in files {
        let loc = FontFaceLoc { path: dir.join(f), ttc_index: 0 };
        if let Some((blob, m)) = caches.resolve_blob(&loc, None) {
            return Some((loc, blob, m));
        }
    }
    None
}

fn load_default(
    fonts: &mut FontDefinitions,
    caches: &mut BlobCache,
    dir: &Path,
    name: &str,
    files: &[&str],
    sync_target: Option<VerticalMetrics>,
) -> (Option<String>, Option<VerticalMetrics>) {
    for f in files {
        let loc = FontFaceLoc { path: dir.join(f), ttc_index: 0 };
        if let Some((blob, m)) = caches.resolve_blob(&loc, sync_target) {
            return (Some(register_font(fonts, name, blob, loc.ttc_index, None)), m);
        }
    }
    (None, None)
}

fn resolve_role_loc(
    custom: Option<&FontFamilyInfo>,
    use_bold: bool,
    dir: &Path,
    default_files: &[&str],
) -> Option<(FontFaceLoc, bool)> {
    if let Some(info) = custom
        && let Some(loc) = if use_bold {
            info.bold.as_ref()
        } else {
            info.regular.as_ref().or(info.bold.as_ref())
        }
    {
        return Some((loc.clone(), true));
    }
    for f in default_files {
        let loc = FontFaceLoc { path: dir.join(f), ttc_index: 0 };
        if loc.path.is_file() {
            return Some((loc, false));
        }
    }
    None
}

fn load_primary(
    fonts: &mut FontDefinitions,
    caches: &mut BlobCache,
    custom_name: &str,
    default_name: &str,
    loc_is_custom: Option<(FontFaceLoc, bool)>,
    sync_target: Option<VerticalMetrics>,
) -> (Option<String>, Option<VerticalMetrics>) {
    let Some((loc, is_custom)) = loc_is_custom else {
        return (None, None);
    };
    let Some((blob, m)) = caches.resolve_blob(&loc, sync_target) else {
        return (None, None);
    };
    let name = if is_custom { custom_name } else { default_name };
    (Some(register_font(fonts, name, blob, loc.ttc_index, None)), m)
}

fn cjk_for_mid(
    fonts: &mut FontDefinitions,
    role_name: &str,
    primary_metrics: Option<VerticalMetrics>,
    target_mid: Option<f32>,
    cjk_name: &Option<String>,
    cjk_blob: Option<&(Arc<Vec<u8>>, u32)>,
    cjk_metrics: Option<VerticalMetrics>,
) -> Option<String> {
    let base = cjk_name.clone()?;
    if let (Some(pm), Some(tmid), Some((blob, idx)), Some(cm)) = (primary_metrics, target_mid, cjk_blob, cjk_metrics)
        && !pm.has_cjk
    {
        let delta = tmid - cm.mid_factor();
        if delta.abs() > 0.015 {
            let tw = FontTweak { y_offset_factor: delta.clamp(-0.4, 0.4), ..Default::default() };
            return Some(register_font(fonts, role_name, Arc::clone(blob), *idx, Some(&tw)));
        }
    }
    Some(base)
}

pub fn apply_choice(ctx: &egui::Context, catalog: Option<&FontCatalog>, choice: &FontChoice) {
    let win_dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let dir = PathBuf::from(win_dir).join("Fonts");
    let mut fonts = FontDefinitions::default();
    let mut caches = BlobCache::default();

    let custom_preview = (!choice.preview.is_empty())
        .then(|| catalog.and_then(|c| c.families.get(&choice.preview)))
        .flatten();
    let custom_editor = (!choice.editor.is_empty())
        .then(|| catalog.and_then(|c| c.families.get(&choice.editor)))
        .flatten();
    let custom_code = (!choice.code.is_empty())
        .then(|| catalog.and_then(|c| c.families.get(&choice.code)))
        .flatten();

    let (cjk, cjk_blob, cjk_metrics) = if let Some((loc, blob, m)) =
        load_default_loc(&mut caches, &dir, &["msyh.ttc", "msyh.ttf", "Deng.ttf", "simhei.ttf", "simsun.ttc"])
    {
        let name = register_font(&mut fonts, "cjk", Arc::clone(&blob), loc.ttc_index, None);
        (Some(name), Some((blob, loc.ttc_index)), m)
    } else {
        (None, None, None)
    };
    let (symbol, _) = load_default(&mut fonts, &mut caches, &dir, "symbol", &["seguisym.ttf"], None);

    let ui_reg_loc = resolve_role_loc(custom_preview, false, &dir, &["segoeui.ttf"]);
    let ui_reg_sync = if ui_reg_loc.as_ref().is_some_and(|(_, c)| *c) { cjk_metrics } else { None };
    let (ui_regular, ui_eff_m) =
        load_primary(&mut fonts, &mut caches, "ui-regular", "segoe", ui_reg_loc, ui_reg_sync);

    let ui_bold_loc = resolve_role_loc(custom_preview, true, &dir, &["segoeuib.ttf"]);
    let ui_bold_sync = if custom_preview.is_some() { cjk_metrics } else { None };
    let (ui_bold, ui_bold_eff_m) = if let Some((loc, is_custom)) = ui_bold_loc
        && let Some((blob, m)) = caches.resolve_blob(&loc, ui_bold_sync)
    {
        let y_delta = ui_eff_m
            .zip(m)
            .map(|(u, b)| u.mid_factor() - b.mid_factor())
            .filter(|d| d.abs() > 0.015)
            .map(|d| d.clamp(-0.4, 0.4))
            .unwrap_or(0.0);
        let unhint = m.is_some_and(|vm| vm.has_cjk);
        let tweak = (unhint || y_delta != 0.0).then(|| FontTweak {
            y_offset_factor: y_delta,
            hinting: unhint.then_some(false),
            ..Default::default()
        });
        let name = if is_custom { "ui-bold" } else { "segoe-bold" };
        (Some(register_font(&mut fonts, name, blob, loc.ttc_index, tweak.as_ref())), m)
    } else {
        (None, None)
    };

    let cjk_bold = if let Some((loc, blob, cbm)) =
        load_default_loc(&mut caches, &dir, &["msyhbd.ttc", "msyhbd.ttf", "Dengb.ttf", "simhei.ttf"])
    {
        let y_delta = if ui_bold_eff_m.is_some_and(|bm| !bm.has_cjk)
            && let (Some(um), Some(cm)) = (ui_eff_m, cbm)
            && (um.mid_factor() - cm.mid_factor()).abs() > 0.015
        {
            (um.mid_factor() - cm.mid_factor()).clamp(-0.4, 0.4)
        } else {
            0.0
        };
        let tw = FontTweak {
            y_offset_factor: y_delta,
            hinting: Some(false),
            ..Default::default()
        };
        Some(register_font(&mut fonts, "cjk-bold", blob, loc.ttc_index, Some(&tw)))
    } else {
        None
    };

    let code_loc = resolve_role_loc(custom_code, false, &dir, &["consola.ttf"]);
    let (mono_code, mono_eff_m) =
        load_primary(&mut fonts, &mut caches, "code-mono", "consolas", code_loc.clone(), cjk_metrics);

    let (code_inline, code_post_m, applied_inline_delta) =
        if let Some((loc, is_custom)) = code_loc
            && let Some((blob, post_m)) = caches.resolve_blob(&loc, cjk_metrics)
        {
            let inline_delta = ui_eff_m
                .zip(post_m)
                .map(|(u, k)| (u.mid_factor() - k.mid_factor()).clamp(-0.35, 0.35))
                .unwrap_or(0.0);
            let tweak = (inline_delta.abs() > 0.025)
                .then(|| FontTweak { y_offset_factor: inline_delta, ..Default::default() });
            let applied = tweak.as_ref().map_or(0.0, |t| t.y_offset_factor);
            let name = if is_custom { "code-inline" } else { "consolas-inline" };
            (Some(register_font(&mut fonts, name, blob, loc.ttc_index, tweak.as_ref())), post_m, applied)
        } else {
            (None, None, 0.0)
        };

    let editor_loc = resolve_role_loc(custom_editor, false, &dir, &["consola.ttf"]);
    let (editor_font, editor_eff_m) =
        load_primary(&mut fonts, &mut caches, "editor-font", "consolas-editor", editor_loc, cjk_metrics);

    let cjk_prop = cjk_for_mid(
        &mut fonts,
        "cjk-prop",
        ui_eff_m,
        ui_eff_m.map(VerticalMetrics::mid_factor),
        &cjk,
        cjk_blob.as_ref(),
        cjk_metrics,
    );
    let cjk_mono = cjk_for_mid(
        &mut fonts,
        "cjk-mono",
        mono_eff_m,
        mono_eff_m.map(VerticalMetrics::mid_factor),
        &cjk,
        cjk_blob.as_ref(),
        cjk_metrics,
    );
    let cjk_editor = cjk_for_mid(
        &mut fonts,
        "cjk-editor",
        editor_eff_m,
        editor_eff_m.map(VerticalMetrics::mid_factor),
        &cjk,
        cjk_blob.as_ref(),
        cjk_metrics,
    );
    let cjk_inline = cjk_for_mid(
        &mut fonts,
        "cjk-inline",
        code_post_m,
        code_post_m.map(|m| m.mid_factor() + applied_inline_delta),
        &cjk,
        cjk_blob.as_ref(),
        cjk_metrics,
    );

    let defaults_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let defaults_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    let build = |front: &[&Option<String>], rest: &[String]| -> Vec<String> {
        let mut v: Vec<String> = front.iter().filter_map(|o| (*o).clone()).collect();
        if let Some(s) = &symbol {
            v.push(s.clone());
        }
        for r in rest {
            if !v.contains(r) {
                v.push(r.clone());
            }
        }
        v
    };

    let prop = build(&[&ui_regular, &cjk_prop], &defaults_prop);
    let bold = build(&[&ui_bold, &cjk_bold, &ui_regular, &cjk_prop], &defaults_prop);
    let monospace = build(&[&mono_code, &cjk_mono], &defaults_mono);
    let editor_family = build(&[&editor_font, &cjk_editor], &defaults_mono);
    let code = build(&[&code_inline, &cjk_inline], &defaults_mono);

    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    fonts.families.insert(FontFamily::Name("editor".into()), editor_family);
    fonts.families.insert(FontFamily::Monospace, monospace);
    fonts.families.insert(FontFamily::Name("code".into()), code);

    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win_font_bytes(file: &str) -> Vec<u8> {
        let win_dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        std::fs::read(PathBuf::from(win_dir).join("Fonts").join(file)).unwrap()
    }

    #[test]
    fn scan_system_fonts_reads_windows_fonts_via_tables() {
        let cat = scan_system_fonts();
        // Windows 下应至少扫描到常用系统字体（如 Segoe UI / Consolas）
        assert!(!cat.families.is_empty());
        assert!(cat.families.values().all(|f| f.regular.is_some() || f.bold.is_some()));
    }

    #[test]
    fn consolas_syncs_vertical_metrics_with_yahei() {
        let consola = win_font_bytes("consola.ttf");
        let msyh = win_font_bytes("msyh.ttc");
        let consola_m = read_vertical_metrics(&consola, 0).expect("consola metrics");
        let cjk_m = read_vertical_metrics(&msyh, 0).expect("msyh metrics");
        assert!(!consola_m.has_cjk);
        assert!(cjk_m.has_cjk);

        let patched = sync_vertical_metrics(&consola, 0, cjk_m).expect("consola should sync");
        let patched_m = read_vertical_metrics(&patched, 0).expect("patched metrics");
        assert!(!patched_m.has_cjk);
        assert!((patched_m.ascent_em - cjk_m.ascent_em).abs() < 1e-3);
        assert!((patched_m.descent_em - cjk_m.descent_em).abs() < 1e-3);
        assert!((patched_m.gap_em - cjk_m.gap_em).abs() < 1e-3);
        assert!((patched_m.mid_factor() - cjk_m.mid_factor()).abs() < 1e-3);
    }

    fn assert_row_aligned(ctx: &egui::Context, family: FontFamily, check_mono: bool) {
        let sample = "├── main.rs          入口: NativeOptions + run_native (约 40 行)";
        let mut galley_opt = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let job = egui::text::LayoutJob::simple(
                sample.to_string(),
                egui::FontId::new(15.0, family.clone()),
                egui::Color32::BLACK,
                f32::INFINITY,
            );
            galley_opt = Some(ui.fonts_mut(|f| f.layout_job(job)));
        });
        out.textures_delta.clear();

        let galley = galley_opt.unwrap();
        assert_eq!(galley.rows.len(), 1);
        let glyphs = &galley.rows[0].row.glyphs;
        assert!(!glyphs.is_empty());

        let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_top, mut max_top) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_bot, mut max_bot) = (f32::INFINITY, f32::NEG_INFINITY);
        for g in glyphs {
            if g.chr == ' ' || g.uv_rect.is_nothing() {
                continue;
            }
            let r = g.logical_rect();
            min_y = min_y.min(g.pos.y);
            max_y = max_y.max(g.pos.y);
            min_top = min_top.min(r.top());
            max_top = max_top.max(r.top());
            min_bot = min_bot.min(r.bottom());
            max_bot = max_bot.max(r.bottom());
        }
        assert!(max_y - min_y <= 0.5, "baseline mismatch in {family:?}: {min_y}..{max_y}");
        assert!(max_top - min_top <= 0.5, "logical top mismatch in {family:?}: {min_top}..{max_top}");
        assert!(max_bot - min_bot <= 0.5, "logical bottom mismatch in {family:?}: {min_bot}..{max_bot}");

        if check_mono {
            let w_i = glyphs.iter().find(|g| g.chr == 'i').unwrap().advance_width;
            let w_m = glyphs.iter().find(|g| g.chr == 'm').unwrap().advance_width;
            let w_sp = glyphs.iter().find(|g| g.chr == ' ').unwrap().advance_width;
            assert!((w_i - w_m).abs() < 1e-3, "not monospace: i={w_i}, m={w_m}");
            assert!((w_i - w_sp).abs() < 1e-3, "not monospace: i={w_i}, space={w_sp}");
        }
    }

    #[test]
    fn mixed_cjk_and_latin_share_baseline_and_logical_rect_in_egui() {
        let ctx = egui::Context::default();
        setup(&ctx);
        for (fam, mono) in [
            (FontFamily::Name("editor".into()), true),
            (FontFamily::Monospace, true),
            (FontFamily::Name("code".into()), true),
        ] {
            assert_row_aligned(&ctx, fam, mono);
        }

        let cat = scan_system_fonts();
        if cat.families.contains_key("Cascadia Mono") {
            let ctx2 = egui::Context::default();
            let choice = FontChoice {
                preview: String::new(),
                editor: "Cascadia Mono".into(),
                code: "Cascadia Mono".into(),
            };
            apply_choice(&ctx2, Some(&cat), &choice);
            for fam in [
                FontFamily::Name("editor".into()),
                FontFamily::Monospace,
                FontFamily::Name("code".into()),
            ] {
                assert_row_aligned(&ctx2, fam, true);
            }
        }

        if cat.families.contains_key("Microsoft YaHei UI") {
            let ctx3 = egui::Context::default();
            let choice = FontChoice {
                preview: String::new(),
                editor: "Microsoft YaHei UI".into(),
                code: "Microsoft YaHei UI".into(),
            };
            apply_choice(&ctx3, Some(&cat), &choice);
            for fam in [
                FontFamily::Name("editor".into()),
                FontFamily::Monospace,
                FontFamily::Name("code".into()),
            ] {
                assert_row_aligned(&ctx3, fam, false);
            }
        }
    }

    fn assert_preview_bold_row_aligned(
        ctx: &egui::Context,
        sample: &str,
    ) -> (Arc<egui::Galley>, Option<Arc<egui::ColorImage>>) {
        let th = crate::markdown::Theme::from_theme(crate::theme::ThemeId::ZenHorizon, 16.0, 1.6, 0.75);
        let doc = crate::markdown::parse(
            sample,
            &th,
            &crate::markdown::ParseOptions { sup_sub: false, front_matter: false },
        );
        let crate::markdown::Block::Paragraph(inl) = &doc.blocks[0] else {
            panic!("expected paragraph");
        };
        let crate::markdown::Chunk::Text(rich) = &inl.chunks[0] else {
            panic!("expected rich text chunk");
        };

        let mut para_galley = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            para_galley = Some(ui.fonts_mut(|f| f.layout_job((*rich.job).clone())));
        });
        let atlas = out.textures_delta.set.iter().find_map(|(id, deltas)| {
            (*id == egui::TextureId::default())
                .then(|| deltas.iter().find(|d| d.pos.is_none()))
                .flatten()
                .map(|d| {
                    let egui::ImageData::Color(c) = &d.image;
                    Arc::clone(c)
                })
        });
        out.textures_delta.clear();

        let galley = para_galley.unwrap();
        assert_eq!(galley.rows.len(), 1);
        let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_top, mut max_top) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_bot, mut max_bot) = (f32::INFINITY, f32::NEG_INFINITY);
        for g in &galley.rows[0].row.glyphs {
            if g.chr == ' ' || g.uv_rect.is_nothing() {
                continue;
            }
            let r = g.logical_rect();
            min_y = min_y.min(g.pos.y);
            max_y = max_y.max(g.pos.y);
            min_top = min_top.min(r.top());
            max_top = max_top.max(r.top());
            min_bot = min_bot.min(r.bottom());
            max_bot = max_bot.max(r.bottom());
        }
        assert!(max_y - min_y <= 0.5, "baseline mismatch: {min_y}..{max_y}");
        assert!(max_top - min_top <= 0.5, "logical top mismatch: {min_top}..{max_top}");
        assert!(max_bot - min_bot <= 0.5, "logical bottom mismatch: {min_bot}..{max_bot}");

        (galley, atlas)
    }

    #[test]
    fn bold_and_regular_preview_share_baseline_and_ink_bounds() {
        let sample = "这是 **重要结论**，也可以写成 __同样加粗__，或 ***非常重要***，English **Bold**。";
        let ctx = egui::Context::default();
        setup(&ctx);
        let (galley, atlas) = assert_preview_bold_row_aligned(&ctx, sample);
        let atlas = atlas.expect("expected default font atlas in textures_delta.set");

        let (mut min_ink_bot, mut max_ink_bot) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut checked = 0usize;
        for g in &galley.rows[0].row.glyphs {
            if !matches!(g.chr, '这' | '是' | '重' | '要' | '同' | '粗') || g.uv_rect.is_nothing() {
                continue;
            }
            let x0 = g.uv_rect.min[0] as usize;
            let x1 = g.uv_rect.max[0] as usize;
            let y0 = g.uv_rect.min[1] as usize;
            let y1 = g.uv_rect.max[1] as usize;
            let mut max_py: Option<usize> = None;
            for py in y0..y1 {
                for px in x0..x1 {
                    if atlas[(px, py)].a() >= 64 {
                        max_py = Some(max_py.map_or(py, |m| m.max(py)));
                    }
                }
            }
            let max_py = max_py.unwrap_or_else(|| panic!("no opaque ink for {:?}", g.chr));
            let ink_bot = g.pos.y + g.uv_rect.offset.y + (max_py + 1 - y0) as f32 / galley.pixels_per_point;
            min_ink_bot = min_ink_bot.min(ink_bot);
            max_ink_bot = max_ink_bot.max(ink_bot);
            checked += 1;
        }
        assert!(checked >= 6, "expected to inspect regular and bold CJK glyphs, got {checked}");
        assert!(
            max_ink_bot - min_ink_bot <= 1.0,
            "CJK visible ink bottom drifted: {min_ink_bot}..{max_ink_bot}"
        );

        let cat = scan_system_fonts();
        for name in ["Microsoft YaHei UI", "Cascadia Mono"] {
            if cat.families.contains_key(name) {
                let ctx_custom = egui::Context::default();
                let choice = FontChoice {
                    preview: name.into(),
                    editor: String::new(),
                    code: String::new(),
                };
                apply_choice(&ctx_custom, Some(&cat), &choice);
                let _ = assert_preview_bold_row_aligned(&ctx_custom, sample);
            }
        }
    }
}
