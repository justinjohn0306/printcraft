//! The last-resort interface fonts: faces already installed on this machine, one per script the
//! embedded faces may not cover.
//!
//! The embedded faces (Inter, egui's defaults, craft-fonts) come first in every family; these only
//! draw characters none of them has, such as an Arabic file name, or Chinese UI labels in a build
//! whose craft-fonts input lacks a Hans face (issue #826). They are read at runtime and never
//! embedded or shipped (AGENTS.md §1.4), and `PDFCRAFT_SYSTEM_FONTS=0` turns them off (published
//! screenshots do).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui::FontData;

/// Larger files are not read: a font path is still untrusted input. CJK collections are big
/// (macOS PingFang, Windows YaHei and Noto CJK are all tens of MB), so the cap is generous.
const MAX_BYTES: u64 = 64 << 20;
/// Faces tried in a collection (`.ttc`).
const MAX_FACES: u32 = 16;
/// Arabic letter alef: an Arabic-script fallback must have it to be worth loading.
const PROBE_ARABIC: char = '\u{0627}';
/// 欢 (U+6B22), Simplified-only: a CJK fallback must have it, which also proves it covers
/// Simplified Chinese rather than only the Han characters shared with Japanese.
const PROBE_CJK: char = '\u{6B22}';

/// The installed fallback faces, read once. Empty when turned off or no candidate fits.
pub fn fallback() -> Vec<Arc<FontData>> {
    static CACHE: OnceLock<Vec<Arc<FontData>>> = OnceLock::new();
    CACHE.get_or_init(load).clone()
}

fn load() -> Vec<Arc<FontData>> {
    if std::env::var_os("PDFCRAFT_SYSTEM_FONTS").is_some_and(|v| v == "0") {
        return Vec::new();
    }
    let mut out: Vec<Arc<FontData>> = Vec::new();
    let mut loaded: Vec<PathBuf> = Vec::new();
    // One face per script: the first candidate that both exists and covers the script's probe.
    for (probe, cands) in script_candidates() {
        for path in cands {
            if loaded.contains(&path) {
                continue; // already loaded for an earlier script; try the next candidate
            }
            if let Some(data) = read(&path, probe) {
                out.push(data);
                loaded.push(path);
                break;
            }
        }
    }
    out
}

/// Well-known locations of broad-coverage faces, grouped by the script each group fills and
/// ordered best first within a group.
fn script_candidates() -> Vec<(char, Vec<PathBuf>)> {
    if cfg!(windows) {
        let dir = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")).map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let font = |f: &str| dir.join("Fonts").join(f);
        vec![
            (PROBE_ARABIC, ["segoeui.ttf", "tahoma.ttf", "arial.ttf"].iter().map(|f| font(f)).collect()),
            // Microsoft YaHei (Simplified), then SimSun, then JhengHei (Traditional) as a last resort.
            (PROBE_CJK, ["msyh.ttc", "msyhl.ttc", "simsun.ttc", "simsun.ttf", "msjh.ttc"].iter().map(|f| font(f)).collect()),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            (
                PROBE_ARABIC,
                ["/System/Library/Fonts/SFArabic.ttf", "/System/Library/Fonts/GeezaPro.ttc", "/System/Library/Fonts/Supplemental/Arial.ttf"]
                    .iter()
                    .map(PathBuf::from)
                    .collect(),
            ),
            // PingFang covers SC/TC/JP; STHeiti and Hiragino Sans GB cover Simplified Chinese too.
            (
                PROBE_CJK,
                [
                    "/System/Library/Fonts/PingFang.ttc",
                    "/System/Library/Fonts/STHeiti Light.ttc",
                    "/System/Library/Fonts/Hiragino Sans GB.ttc",
                    "/Library/Fonts/Arial Unicode.ttf",
                ]
                .iter()
                .map(PathBuf::from)
                .collect(),
            ),
        ]
    } else {
        let dirs = ["/usr/share/fonts", "/usr/local/share/fonts"];
        let find = |files: &[&str]| -> Vec<PathBuf> { dirs.iter().flat_map(|d| files.iter().map(move |f| Path::new(d).join(f))).collect() };
        vec![
            (
                PROBE_ARABIC,
                find(&[
                    "truetype/noto/NotoSansArabic-Regular.ttf",
                    "noto/NotoSansArabic-Regular.ttf",
                    "google-noto/NotoSansArabic-Regular.ttf",
                    "truetype/dejavu/DejaVuSans.ttf",
                    "TTF/DejaVuSans.ttf",
                    "dejavu/DejaVuSans.ttf",
                    "dejavu-sans-fonts/DejaVuSans.ttf",
                ]),
            ),
            (
                PROBE_CJK,
                find(&[
                    "opentype/noto/NotoSansCJK-Regular.ttc",
                    "opentype/noto/NotoSansCJKsc-Regular.otf",
                    "truetype/noto/NotoSansCJK-Regular.ttc",
                    "noto-cjk/NotoSansCJK-Regular.ttc",
                    "google-noto-cjk/NotoSansCJK-Regular.ttc",
                    "wenquanyi/wqy-microhei/wqy-microhei.ttc",
                    "truetype/wqy/wqy-microhei.ttc",
                ]),
            ),
        ]
    }
}

fn read(path: &Path, probe: char) -> Option<Arc<FontData>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let index = face_with(&bytes, probe)?;
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    log::info!("interface font fallback: {} (face {index})", path.display());
    Some(Arc::new(data))
}

/// The first face of the file that parses and maps `c`. egui parses fonts with the same skrifa,
/// so a face accepted here is one it can load.
fn face_with(bytes: &[u8], c: char) -> Option<u32> {
    use skrifa::MetadataProvider as _;
    (0..MAX_FACES).find(|&index| skrifa::FontRef::from_index(bytes, index).is_ok_and(|font| font.charmap().map(c).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_and_missing_files_are_skipped() {
        assert_eq!(face_with(b"", PROBE_ARABIC), None);
        assert_eq!(face_with(b"not a font at all", PROBE_ARABIC), None);
        assert_eq!(face_with(&[0u8; 4096], PROBE_ARABIC), None);
        assert!(read(Path::new("definitely/not/here.ttf"), PROBE_ARABIC).is_none());
        // A directory is not a font.
        assert!(read(&std::env::temp_dir(), PROBE_ARABIC).is_none());
    }

    #[test]
    fn a_face_without_the_probe_is_rejected() {
        // Inter is Latin, Greek and Cyrillic only.
        let inter = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
        assert_eq!(face_with(inter, PROBE_ARABIC), None);
        assert_eq!(face_with(inter, PROBE_CJK), None);
        assert_eq!(face_with(inter, 'A'), Some(0));
    }

    #[test]
    fn candidates_are_absolute_font_files() {
        let groups = script_candidates();
        // One group per script we try to fill, each with a probe and candidates.
        assert!(groups.iter().any(|(p, _)| *p == PROBE_ARABIC));
        assert!(groups.iter().any(|(p, _)| *p == PROBE_CJK));
        let all: Vec<PathBuf> = groups.into_iter().flat_map(|(_, c)| c).collect();
        assert!(!all.is_empty());
        assert!(all.iter().all(|p| p.extension().is_some_and(|e| e == "ttf" || e == "ttc" || e == "otf")));
    }
}
