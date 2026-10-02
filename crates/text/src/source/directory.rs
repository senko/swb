//! A font source with the fonts of one directory and an explicit family map.
//! It does not use fontconfig, so results do not depend on the machine.

use std::collections::HashMap;
use std::path::Path;

use skrifa::FontRef;
use skrifa::charmap::MappingIndex;

use crate::error::TextError;
use crate::face::{self, FaceDesc, FamilyDesc};
use crate::matching::{self, Candidate, Desired};
use crate::query::{FontStyle, GenericFamily, GenericFamilyMap};
use crate::source::FontSource;

/// One face of the directory with its character mapping.
struct DirFace {
    desc: FaceDesc,
    mapping: MappingIndex,
}

impl DirFace {
    fn covers(&self, c: char) -> bool {
        let Some(data) = &self.desc.data else {
            return false;
        };
        FontRef::from_index(data, self.desc.index)
            .is_ok_and(|font| self.mapping.charmap(&font).map(c).is_some())
    }
}

/// The fonts of one directory.
pub(crate) struct DirectorySource {
    /// Faces sorted by file path, then face index.
    faces: Vec<DirFace>,
    /// Lowercase family name to (family name as in the font, face indices).
    families: HashMap<String, (String, Vec<usize>)>,
    map: GenericFamilyMap,
}

impl DirectorySource {
    /// Reads all `.ttf`, `.otf`, `.ttc` and `.otc` files in `dir` (not
    /// recursive).
    pub(crate) fn new(dir: &Path, map: GenericFamilyMap) -> Result<Self, TextError> {
        let io_error = |source| TextError::Io {
            path: dir.to_owned(),
            source,
        };
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            let is_font = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                ["ttf", "otf", "ttc", "otc"]
                    .iter()
                    .any(|ext| e.eq_ignore_ascii_case(ext))
            });
            if is_font && path.is_file() {
                paths.push(path);
            }
        }
        paths.sort();

        let mut source = DirectorySource {
            faces: Vec::new(),
            families: HashMap::new(),
            map,
        };
        for path in paths {
            let data = face::read_font_file(&path)?;
            for (names, desc) in face::describe_file(&path, &data) {
                source.add_face(names, desc);
            }
        }
        if source.faces.is_empty() {
            return Err(TextError::NoFonts(dir.to_owned()));
        }
        Ok(source)
    }

    fn add_face(&mut self, names: Vec<String>, desc: FaceDesc) {
        let Some(mapping) = desc
            .data
            .as_ref()
            .and_then(|data| FontRef::from_index(data, desc.index).ok())
            .map(|font| MappingIndex::new(&font))
        else {
            return;
        };
        let index = self.faces.len();
        for name in names {
            self.families
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| (name, Vec::new()))
                .1
                .push(index);
        }
        self.faces.push(DirFace { desc, mapping });
    }

    /// The family with exactly this name (ignoring ASCII case).
    fn family(&self, name: &str) -> Option<FamilyDesc> {
        let (family_name, indices) = self.families.get(&name.to_ascii_lowercase())?;
        Some(FamilyDesc {
            name: family_name.clone(),
            faces: indices
                .iter()
                .map(|&i| {
                    let mut desc = self.faces[i].desc.clone();
                    desc.family.clone_from(family_name);
                    desc
                })
                .collect(),
        })
    }

    /// The face of a family that is closest to regular (weight 400, normal
    /// style and stretch). System fallback in Chromium also prefers regular
    /// faces, because fontconfig sorts them first.
    fn regular_face(&self, name: &str) -> Option<usize> {
        let (_, indices) = self.families.get(&name.to_ascii_lowercase())?;
        let candidates: Vec<Candidate> = indices
            .iter()
            .map(|&i| {
                let desc = &self.faces[i].desc;
                Candidate {
                    weight: desc.weight.unwrap_or((400.0, 400.0)),
                    stretch: (desc.stretch, desc.stretch),
                    style: desc.style,
                }
            })
            .collect();
        let desired = Desired {
            weight: 400.0,
            stretch: 100.0,
            style: FontStyle::Normal,
        };
        matching::best_match(&candidates, desired).map(|i| indices[i])
    }
}

impl FontSource for DirectorySource {
    fn named_family(&mut self, name: &str) -> Option<FamilyDesc> {
        self.family(name)
            .or_else(|| self.map.alias(name).and_then(|alias| self.family(alias)))
    }

    fn generic_family(&mut self, generic: GenericFamily) -> Option<FamilyDesc> {
        self.family(self.map.generic(generic))
    }

    fn default_family(&mut self) -> Option<FamilyDesc> {
        // Blink's standard font is serif; see `GenericFamilyMap`.
        let serif = self.map.generic(GenericFamily::Serif).to_owned();
        self.family(&serif)
            .or_else(|| self.family(&self.map.default_family.clone()))
            .or_else(|| self.faces.first().and_then(|f| self.family(&f.desc.family)))
    }

    /// Candidate order: the regular face of each fallback family (positions
    /// `0..fallback.len()`), then every face in order of file path.
    fn fallback_face(
        &mut self,
        c: char,
        _language: &str,
        from: usize,
    ) -> Option<(usize, FaceDesc)> {
        let this = &*self;
        let families = this.map.fallback.len();
        let family_faces = (from.min(families)..families).filter_map(|position| {
            Some((position, this.regular_face(&this.map.fallback[position])?))
        });
        let all_faces = (from.saturating_sub(families)..this.faces.len())
            .map(|index| (families + index, index));
        family_faces
            .chain(all_faces)
            .find(|&(_, index)| this.faces[index].covers(c))
            .map(|(position, index)| (position, this.faces[index].desc.clone()))
    }
}
