//! The long English names of time zones as `toString` prints them
//! (`Central European Summer Time`), from the data of CLDR 48 (ADR 0027
//! section 6; `zone_names_data.rs` is generated).
//!
//! The rules are those of UTS #35 part 4 ("Using Time Zone Names",
//! <https://www.unicode.org/reports/tr35/tr35-dates.html#Using_Time_Zone_Names>)
//! for the long specific non-location name (the pattern `zzzz`):
//!
//! 1. A zone name is first mapped to the identifier of CLDR (the aliases
//!    of `bcp47/timezone.xml`, such as `Asia/Kolkata` for `Asia/Calcutta`).
//! 2. A name of the zone itself (`<zone><long>`; only `Etc/UTC`,
//!    `Europe/London` and `Europe/Dublin` have one) wins.
//! 3. Else the name of the metazone that the zone has now
//!    (`metaZones.xml`), the standard or the daylight name by whether the
//!    zone is in daylight saving time at the printed time. Chromium takes
//!    the metazone of the present, not of the printed date (the name of
//!    Asia/Almaty in 2005 is "Kazakhstan Time", the name since 2024).
//! 4. Without a name, the localized GMT format: `GMT+01:00`.
//!
//! Chromium 148 follows these rules (measured over the zones of the
//! system database, `tools/probes/js-date.json` and ADR 0027).

use super::zone_names_data::{
    METAZONES, ZONE_ENDS, ZONE_METAZONE, ZONE_OVERRIDE_OF, ZONE_OVERRIDES, ZONE_TEXT,
};

/// The data of one zone name: its metazone and its own names.
#[derive(Clone, Copy, Debug)]
pub(super) struct ZoneNames {
    /// The metazone plus one, 0 for none.
    metazone: usize,
    /// The own names plus one, 0 for none.
    own: usize,
}

/// The zone name at position `index` of the sorted table.
fn name_at(index: usize) -> Option<&'static str> {
    let end = usize::from(*ZONE_ENDS.get(index)?);
    let start = match index.checked_sub(1) {
        Some(before) => usize::from(*ZONE_ENDS.get(before)?),
        None => 0,
    };
    // Each name is followed by a line feed.
    ZONE_TEXT.get(start..end.checked_sub(1)?)
}

/// The names of the zone `name` (an identifier of the time zone database,
/// such as `Europe/Berlin`), or `None` for a name that CLDR does not have.
pub(super) fn find(name: &str) -> Option<ZoneNames> {
    let (mut low, mut high) = (0, ZONE_ENDS.len());
    while low < high {
        let middle = low + (high - low) / 2;
        match name_at(middle)?.cmp(name) {
            std::cmp::Ordering::Equal => {
                return Some(ZoneNames {
                    metazone: usize::from(*ZONE_METAZONE.get(middle)?),
                    own: usize::from(*ZONE_OVERRIDE_OF.get(middle)?),
                });
            }
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
        }
    }
    None
}

impl ZoneNames {
    /// The long name for a zone in standard time or in daylight saving
    /// time, `None` if CLDR has no such name (the caller then prints the
    /// GMT format).
    pub(super) fn long_name(self, daylight: bool) -> Option<&'static str> {
        let pick = |(standard, saving): &(&'static str, &'static str)| {
            let name = if daylight { saving } else { standard };
            (!name.is_empty()).then_some(*name)
        };
        let own = self
            .own
            .checked_sub(1)
            .and_then(|i| ZONE_OVERRIDES.get(i))
            .and_then(pick);
        own.or_else(|| {
            let metazone = self.metazone.checked_sub(1)?;
            METAZONES.get(metazone).and_then(pick)
        })
    }
}

/// The localized GMT format of an offset in seconds: `GMT+01:00`, and
/// `GMT+00:53:28` for an offset with seconds (UTS #35 part 4, "GMT
/// format", the `en` patterns `GMT{0}` and `+HH:mm;-HH:mm`).
pub(super) fn gmt_format(offset_seconds: i32) -> String {
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let total = offset_seconds.unsigned_abs();
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if seconds == 0 {
        format!("GMT{sign}{hours:02}:{minutes:02}")
    } else {
        format!("GMT{sign}{hours:02}:{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(zone: &str, daylight: bool) -> Option<&'static str> {
        find(zone)?.long_name(daylight)
    }

    #[test]
    fn metazone_names() {
        assert_eq!(
            name("Europe/Berlin", true),
            Some("Central European Summer Time")
        );
        assert_eq!(
            name("Europe/Berlin", false),
            Some("Central European Standard Time")
        );
        assert_eq!(name("Asia/Calcutta", false), Some("India Standard Time"));
        assert_eq!(name("Asia/Kolkata", false), Some("India Standard Time"));
        assert_eq!(
            name("America/Los_Angeles", true),
            Some("Pacific Daylight Time")
        );
        assert_eq!(name("Asia/Almaty", false), Some("Kazakhstan Time"));
    }

    #[test]
    fn zone_names_win_over_metazone_names() {
        assert_eq!(name("Europe/London", true), Some("British Summer Time"));
        assert_eq!(name("Europe/London", false), Some("Greenwich Mean Time"));
        assert_eq!(name("Europe/Dublin", true), Some("Irish Standard Time"));
        assert_eq!(name("Etc/UTC", false), Some("Coordinated Universal Time"));
        assert_eq!(name("Etc/GMT", false), Some("Greenwich Mean Time"));
    }

    #[test]
    fn no_name_means_the_gmt_format() {
        // A metazone with a standard name only, in daylight saving time.
        assert_eq!(name("Antarctica/Troll", true), None);
        // Casablanca has no metazone now (its last one ended in 2018).
        assert_eq!(name("Africa/Casablanca", false), None);
        // Not a zone of CLDR.
        assert!(find("Etc/GMT+5").is_none());
        assert!(find("EST5EDT,M3.2.0,M11.1.0").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn the_table_is_sorted_and_consistent() {
        let names: Vec<&str> = (0..ZONE_ENDS.len()).filter_map(name_at).collect();
        assert_eq!(names.len(), ZONE_ENDS.len());
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(ZONE_METAZONE.len(), names.len());
        assert_eq!(ZONE_OVERRIDE_OF.len(), names.len());
        assert!(
            ZONE_METAZONE
                .iter()
                .all(|m| usize::from(*m) <= METAZONES.len())
        );
        assert!(
            ZONE_OVERRIDE_OF
                .iter()
                .all(|o| usize::from(*o) <= ZONE_OVERRIDES.len())
        );
        assert!(ZONE_TEXT.ends_with(names.last().copied().unwrap_or("")));
    }

    #[test]
    fn gmt_format_of_offsets() {
        assert_eq!(gmt_format(0), "GMT+00:00");
        assert_eq!(gmt_format(3600), "GMT+01:00");
        assert_eq!(gmt_format(-5 * 3600), "GMT-05:00");
        assert_eq!(gmt_format(5 * 3600 + 45 * 60), "GMT+05:45");
        assert_eq!(gmt_format(-(3 * 3600 + 30 * 60)), "GMT-03:30");
        assert_eq!(gmt_format(3208), "GMT+00:53:28");
    }
}
