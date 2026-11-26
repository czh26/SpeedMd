//! 系统字体扫描（按偏移读取表目录与 name / OS/2 表）与按选择加载字体。

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontTweak};
use read_fonts::{
    tables::{
        name::{Name, NameId},
        os2::{Os2, SelectionFlags},
    },
    FontData as RfFontData, FontRead,
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

fn read_font_blob(cache: &mut HashMap<PathBuf, Arc<Vec<u8>>>, path: &Path) -> Option<Arc<Vec<u8>>> {
    if let Some(b) = cache.get(path) {
        return Some(Arc::clone(b));
    }
    let bytes = std::fs::read(path).ok()?;
    let arc = Arc::new(bytes);
    cache.insert(path.to_path_buf(), Arc::clone(&arc));
    Some(arc)
}

fn load_loc(
    fonts: &mut FontDefinitions,
    cache: &mut HashMap<PathBuf, Arc<Vec<u8>>>,
    name: &str,
    loc: &FontFaceLoc,
    tweak: Option<&FontTweak>,
) -> Option<String> {
    let blob = read_font_blob(cache, &loc.path)?;
    let mut fd = FontData::from_blob(blob, loc.ttc_index);
    if let Some(tw) = tweak {
        fd = fd.tweak(tw.clone());
    }
    fonts.font_data.insert(name.into(), Arc::new(fd));
    Some(name.to_string())
}

fn load_default(
    fonts: &mut FontDefinitions,
    cache: &mut HashMap<PathBuf, Arc<Vec<u8>>>,
    dir: &Path,
    name: &str,
    files: &[&str],
    tweak: Option<&FontTweak>,
) -> Option<String> {
    for f in files {
        let loc = FontFaceLoc { path: dir.join(f), ttc_index: 0 };
        if let Some(n) = load_loc(fonts, cache, name, &loc, tweak) {
            return Some(n);
        }
    }
    None
}

pub fn apply_choice(ctx: &egui::Context, catalog: Option<&FontCatalog>, choice: &FontChoice) {
    let win_dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let dir = PathBuf::from(win_dir).join("Fonts");
    let mut fonts = FontDefinitions::default();
    let mut cache: HashMap<PathBuf, Arc<Vec<u8>>> = HashMap::new();

    let custom_preview = (!choice.preview.is_empty())
        .then(|| catalog.and_then(|c| c.families.get(&choice.preview)))
        .flatten();
    let custom_editor = (!choice.editor.is_empty())
        .then(|| catalog.and_then(|c| c.families.get(&choice.editor)))
        .flatten();
    let custom_code = (!choice.code.is_empty())
        .then(|| catalog.and_then(|c| c.families.get(&choice.code)))
        .flatten();

    let ui_regular = custom_preview
        .and_then(|info| info.regular.as_ref().or(info.bold.as_ref()))
        .and_then(|loc| load_loc(&mut fonts, &mut cache, "ui-regular", loc, None))
        .or_else(|| load_default(&mut fonts, &mut cache, &dir, "segoe", &["segoeui.ttf"], None));

    let ui_bold = custom_preview
        .and_then(|info| info.bold.as_ref())
        .and_then(|loc| load_loc(&mut fonts, &mut cache, "ui-bold", loc, None))
        .or_else(|| load_default(&mut fonts, &mut cache, &dir, "segoe-bold", &["segoeuib.ttf"], None));

    let cjk = load_default(
        &mut fonts,
        &mut cache,
        &dir,
        "cjk",
        &["msyh.ttc", "msyh.ttf", "Deng.ttf", "simhei.ttf", "simsun.ttc"],
        None,
    );
    let cjk_bold = load_default(
        &mut fonts,
        &mut cache,
        &dir,
        "cjk-bold",
        &["msyhbd.ttc", "msyhbd.ttf", "Dengb.ttf", "simhei.ttf"],
        None,
    );
    let symbol = load_default(&mut fonts, &mut cache, &dir, "symbol", &["seguisym.ttf"], None);

    let mono_code = custom_code
        .and_then(|info| info.regular.as_ref().or(info.bold.as_ref()))
        .and_then(|loc| load_loc(&mut fonts, &mut cache, "code-mono", loc, None))
        .or_else(|| load_default(&mut fonts, &mut cache, &dir, "consolas", &["consola.ttf"], None));

    let inline_tweak = FontTweak { y_offset_factor: 0.25, ..Default::default() };
    let code_inline = custom_code
        .and_then(|info| info.regular.as_ref().or(info.bold.as_ref()))
        .and_then(|loc| load_loc(&mut fonts, &mut cache, "code-inline", loc, Some(&inline_tweak)))
        .or_else(|| load_default(&mut fonts, &mut cache, &dir, "consolas-inline", &["consola.ttf"], Some(&inline_tweak)));

    let editor_font = custom_editor
        .and_then(|info| info.regular.as_ref().or(info.bold.as_ref()))
        .and_then(|loc| load_loc(&mut fonts, &mut cache, "editor-font", loc, None))
        .or_else(|| load_default(&mut fonts, &mut cache, &dir, "consolas-editor", &["consola.ttf"], None));

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

    let prop = build(&[&ui_regular, &cjk], &defaults_prop);
    let bold = build(&[&ui_bold, &cjk_bold, &ui_regular, &cjk], &defaults_prop);
    let mono_rest = {
        let mut v = defaults_mono.clone();
        if let Some(c) = &cjk {
            let pos = 1.min(v.len());
            v.insert(pos, c.clone());
        }
        v
    };
    let monospace = build(&[&mono_code], &mono_rest);
    let editor_family = build(&[&editor_font], &mono_rest);
    let code = build(&[&code_inline, &cjk], &defaults_mono);

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

    #[test]
    fn scan_system_fonts_reads_windows_fonts_via_tables() {
        let cat = scan_system_fonts();
        // Windows 下应至少扫描到常用系统字体（如 Segoe UI / Consolas）
        assert!(!cat.families.is_empty());
        assert!(cat.families.values().all(|f| f.regular.is_some() || f.bold.is_some()));
    }
}
