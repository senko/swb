//! Font faces: their descriptions (from a font source) and the parsed data
//! that shaping, metrics and rasterization need.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harfrust::ShaperData;
use skrifa::charmap::MappingIndex;
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider, Tag};

use crate::error::TextError;
use crate::matching::FaceStyle;

/// A face as a font source describes it, before its file is parsed.
#[derive(Clone, Debug)]
pub(crate) struct FaceDesc {
    pub(crate) path: PathBuf,
    pub(crate) index: u32,
    /// The family name under which the source found the face.
    pub(crate) family: String,
    pub(crate) style: FaceStyle,
    /// CSS weight range: the `wght` axis range for variable fonts, a single
    /// value otherwise. `None` if unknown until the file is parsed (variable
    /// fonts in fontconfig).
    pub(crate) weight: Option<(f32, f32)>,
    /// CSS stretch percentage.
    pub(crate) stretch: f32,
    /// The file contents, if the source already read them.
    pub(crate) data: Option<Arc<[u8]>>,
}

/// A family and its faces, as a font source returns it.
#[derive(Clone, Debug)]
pub(crate) struct FamilyDesc {
    pub(crate) name: String,
    pub(crate) faces: Vec<FaceDesc>,
}

/// The range of a variation axis in user units.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct AxisRange {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

/// A parsed face. The font data is shared; `FontRef`s are created on demand
/// because they borrow the data.
pub(crate) struct LoadedFace {
    data: Arc<[u8]>,
    index: u32,
    mapping: MappingIndex,
    pub(crate) shaper: ShaperData,
    pub(crate) units_per_em: u16,
    /// The weight of the default instance (OS/2 `usWeightClass`).
    pub(crate) weight: f32,
    /// Bit `n` is set if the face maps ASCII character `n`. Control
    /// characters count as mapped.
    ascii: u128,
    pub(crate) wght: Option<AxisRange>,
    /// The face has color glyph tables (COLR, CBDT or sbix).
    pub(crate) has_color: bool,
}

impl LoadedFace {
    /// Parses face `index` of `data`.
    pub(crate) fn new(path: &Path, data: &Arc<[u8]>, index: u32) -> Result<Self, TextError> {
        let invalid = |reason: String| TextError::InvalidFont {
            path: path.to_owned(),
            index,
            reason,
        };
        let font = FontRef::from_index(data, index).map_err(|e| invalid(e.to_string()))?;
        let units_per_em = font
            .head()
            .map_err(|e| invalid(format!("head table: {e}")))?
            .units_per_em();
        if !(16..=16384).contains(&units_per_em) {
            return Err(invalid(format!("bad unitsPerEm {units_per_em}")));
        }
        let mapping = MappingIndex::new(&font);
        let charmap = mapping.charmap(&font);
        if !charmap.has_map() {
            return Err(invalid("no usable cmap subtable".to_owned()));
        }
        let mut ascii = 0u128;
        for c in 0u8..128 {
            if c < 0x20 || c == 0x7f || charmap.map(c).is_some() {
                ascii |= 1 << c;
            }
        }
        let wght = font
            .axes()
            .get_by_tag(Tag::new(b"wght"))
            .map(|axis| AxisRange {
                min: axis.min_value(),
                max: axis.max_value(),
            });
        let has_color = [b"COLR", b"CBDT", b"sbix"]
            .iter()
            .any(|tag| font.table_data(Tag::new(tag)).is_some());
        let shaper = ShaperData::new(&font);
        Ok(LoadedFace {
            data: data.clone(),
            index,
            mapping,
            shaper,
            units_per_em,
            weight: font.attributes().weight.value(),
            ascii,
            wght,
            has_color,
        })
    }

    /// A reference to the parsed font. `new` already checked that the data
    /// parses, so `None` does not happen in practice; callers treat it as a
    /// font without glyphs.
    pub(crate) fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index).ok()
    }

    /// The nominal glyph for `c`.
    pub(crate) fn glyph(&self, c: char) -> Option<GlyphId> {
        let font = self.font()?;
        self.mapping.charmap(&font).map(c)
    }

    /// True if the face has a glyph for `c`.
    pub(crate) fn covers(&self, c: char) -> bool {
        let code = c as u32;
        if code < 128 {
            return self.ascii & (1 << code) != 0;
        }
        self.glyph(c).is_some()
    }

    /// True if the face has glyphs for all non-control ASCII characters of
    /// `text`. `text` must be ASCII.
    pub(crate) fn covers_ascii(&self, text: &str) -> bool {
        text.bytes().all(|b| b < 128 && self.ascii & (1 << b) != 0)
    }
}

/// Largest `cmap` table that [`read_cmap_table`] accepts.
const MAX_CMAP_LEN: u32 = 16 << 20;

/// Reads only the `cmap` table of face `index` of a font file. System
/// fallback uses this to test character coverage without reading whole
/// files. Parses the collection header and the table directory
/// (<https://learn.microsoft.com/en-us/typography/opentype/spec/otff#table-directory>).
pub(crate) fn read_cmap_table(path: &Path, index: u32) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};

    fn be32(bytes: &[u8]) -> u32 {
        bytes
            .get(..4)
            .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    let mut face_offset = 0u64;
    if &header[..4] == b"ttcf" {
        if index >= be32(&header[8..]) {
            return None;
        }
        let mut offset = [0u8; 4];
        file.seek(SeekFrom::Start(12 + 4 * u64::from(index))).ok()?;
        file.read_exact(&mut offset).ok()?;
        face_offset = u64::from(be32(&offset));
        file.seek(SeekFrom::Start(face_offset)).ok()?;
        file.read_exact(&mut header).ok()?;
    } else if index != 0 {
        return None;
    }
    let num_tables = usize::from(u16::from_be_bytes([header[4], header[5]]));
    let mut records = vec![0u8; 16 * num_tables];
    file.seek(SeekFrom::Start(face_offset + 12)).ok()?;
    file.read_exact(&mut records).ok()?;
    let record = records
        .as_chunks::<16>()
        .0
        .iter()
        .find(|r| &r[..4] == b"cmap")?;
    let (offset, len) = (be32(&record[8..]), be32(&record[12..]));
    if len > MAX_CMAP_LEN {
        return None;
    }
    let mut table = vec![0u8; len as usize];
    file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
    file.read_exact(&mut table).ok()?;
    Some(table)
}

/// True if a `cmap` table (from [`read_cmap_table`]) maps `c` to a glyph
/// other than `.notdef`. Uses the same subtable choice as shaping. Unlike
/// [`LoadedFace::covers`], it does not look up U+0000..U+00FF in the
/// U+F000 range of symbol fonts, so symbol fonts are not fallback fonts for
/// those characters.
pub(crate) fn cmap_covers(table: &[u8], c: char) -> bool {
    use skrifa::raw::{FontData, FontRead, tables::cmap::Cmap};

    Cmap::read(FontData::new(table))
        .ok()
        .and_then(|cmap| cmap.best_subtable())
        .and_then(|(_, _, subtable)| subtable.map_codepoint(c))
        .is_some_and(|glyph| glyph != GlyphId::NOTDEF)
}

/// Reads a whole font file.
pub(crate) fn read_font_file(path: &Path) -> Result<Arc<[u8]>, TextError> {
    std::fs::read(path)
        .map(Arc::from)
        .map_err(|source| TextError::Io {
            path: path.to_owned(),
            source,
        })
}

/// Describes every face in a font file. Returns the family names of each
/// face (typographic family first, then the legacy family name) with its
/// description. Faces that do not parse are skipped with a warning.
pub(crate) fn describe_file(path: &Path, data: &Arc<[u8]>) -> Vec<(Vec<String>, FaceDesc)> {
    let count = match skrifa::raw::FileRef::new(data) {
        Ok(skrifa::raw::FileRef::Font(_)) => 1,
        Ok(skrifa::raw::FileRef::Collection(collection)) => collection.len(),
        Err(e) => {
            log::warn!("skipping font {}: {e}", path.display());
            return Vec::new();
        }
    };
    (0..count)
        .filter_map(|index| describe_face(path, data, index))
        .collect()
}

fn describe_face(path: &Path, data: &Arc<[u8]>, index: u32) -> Option<(Vec<String>, FaceDesc)> {
    let font = match FontRef::from_index(data, index) {
        Ok(font) => font,
        Err(e) => {
            log::warn!("skipping font {} face {index}: {e}", path.display());
            return None;
        }
    };
    let mut names = Vec::new();
    for id in [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME] {
        if let Some(name) = font.localized_strings(id).english_or_first() {
            let name = name.to_string();
            if !name.is_empty() && !names.iter().any(|n: &String| n.eq_ignore_ascii_case(&name)) {
                names.push(name);
            }
        }
    }
    let Some(family) = names.first().cloned() else {
        log::warn!(
            "skipping font {} face {index}: no family name",
            path.display()
        );
        return None;
    };
    let attributes = font.attributes();
    let style = match attributes.style {
        skrifa::attribute::Style::Normal => FaceStyle::Normal,
        skrifa::attribute::Style::Italic => FaceStyle::Italic,
        skrifa::attribute::Style::Oblique(_) => FaceStyle::Oblique,
    };
    let weight = attributes.weight.value();
    let weight = font
        .axes()
        .get_by_tag(Tag::new(b"wght"))
        .map_or((weight, weight), |axis| {
            (axis.min_value(), axis.max_value())
        });
    let desc = FaceDesc {
        path: path.to_owned(),
        index,
        family,
        style,
        weight: Some(weight),
        stretch: attributes.stretch.percentage(),
        data: Some(data.clone()),
    };
    Some((names, desc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/fonts")
            .join(name)
    }

    #[test]
    fn cmap_probe_reads_coverage() {
        let table = read_cmap_table(&fixture("LiberationSans-Regular.ttf"), 0).unwrap();
        assert!(cmap_covers(&table, 'A'));
        assert!(cmap_covers(&table, 'Ж'));
        assert!(!cmap_covers(&table, '☃'));
        let table = read_cmap_table(&fixture("DejaVuSans.ttf"), 0).unwrap();
        assert!(cmap_covers(&table, '☃'));
        // A plain font file has only face 0.
        assert!(read_cmap_table(&fixture("DejaVuSans.ttf"), 1).is_none());
        assert!(read_cmap_table(&fixture("missing.ttf"), 0).is_none());
    }

    /// A `cmap` table with one format 4 subtable (Windows, Unicode BMP)
    /// that maps U+0041 to glyph 0 and U+0042 to glyph 1.
    fn cmap_with_notdef_mapping() -> Vec<u8> {
        let header = [0, 1, 3, 1, 0, 12];
        // Format 4, length 32, two segments: 0x41..=0x42 with idDelta
        // 0xFFBF (0x41 + 0xFFBF = 0 modulo 2^16), and the final 0xFFFF
        // segment.
        let subtable = [
            4, 32, 0, 4, 4, 1, 0, 0x42, 0xFFFF, 0, 0x41, 0xFFFF, 0xFFBF, 1, 0, 0,
        ];
        header
            .iter()
            .chain(&subtable)
            .flat_map(|word: &u16| word.to_be_bytes())
            .collect()
    }

    #[test]
    fn cmap_probe_ignores_notdef_mappings() {
        let table = cmap_with_notdef_mapping();
        assert!(!cmap_covers(&table, 'A'));
        assert!(cmap_covers(&table, 'B'));
        assert!(!cmap_covers(&table, 'C'));
    }

    #[test]
    fn describes_faces() {
        let path = fixture("LiberationSerif-BoldItalic.ttf");
        let data = read_font_file(&path).unwrap();
        let faces = describe_file(&path, &data);
        assert_eq!(faces.len(), 1);
        let (names, desc) = &faces[0];
        assert_eq!(names, &vec!["Liberation Serif".to_owned()]);
        assert_eq!(desc.weight, Some((700.0, 700.0)));
        assert_eq!(desc.style, FaceStyle::Italic);
        assert_eq!(desc.stretch, 100.0);
    }

    #[test]
    fn loaded_face_coverage() {
        let path = fixture("LiberationMono-Regular.ttf");
        let data = read_font_file(&path).unwrap();
        let face = LoadedFace::new(&path, &data, 0).unwrap();
        assert_eq!(face.units_per_em, 2048);
        assert!(face.covers('a') && face.covers('\n') && face.covers('Ω'));
        assert!(!face.covers('☃'));
        assert!(face.covers_ascii("plain text\t"));
        assert!(face.wght.is_none());
        assert!(!face.has_color);
        assert!(LoadedFace::new(&path, &data, 1).is_err());
        let garbage: Arc<[u8]> = Arc::from(&b"not a font"[..]);
        assert!(LoadedFace::new(&path, &garbage, 0).is_err());
    }
}
