//! The NFSU2 player profile: which performance parts a car has, what they add up to, and the vinyl
//! on it.
//!
//! Found where the game writes it under a Wine prefix:
//! `users/<user>/AppData/Local/NFS Underground 2/<profile>/<profile>` — 54,966 bytes, magic `20CM`,
//! written when the game leaves the garage rather than continuously.
//!
//! **Locked by experiment, on one profile.** Eight purchases were made one at a time and the file
//! diffed after each: the first wrote 388 bytes, every one after it 11–13, the digest at `+0x14`
//! included. That is what makes the two regions below readable rather than guessed:
//!
//! * A contiguous array of `f32` from [`TUNING_AT`], one slot per upgrade category. Each is the
//!   **sum of what is installed** there, and every product carries its own weight — measured
//!   `0 → 0.33 → 0.66 → 0.99 → 1.32` for the gearbox category (steps of 0.33), `0 → 1 → 2` for
//!   nitrous, `0 → 0.21 → 0.51` for the engine. It is *not* a 0..1 fill; that model was held for two
//!   rounds and the file disproved it.
//! * A block of flag bytes from [`INSTALLED_AT`], **one per product**, grouped by category.
//!   Products that share a slot replace one another: buying nitrous level 2 moved the flag from
//!   `+0x0BFC` to `+0x0BFD`, and swapping a differential for its next level moved `+0x0BF8` to
//!   `+0x0BF9` — that one was predicted before the purchase and is the strongest result in the set.
//! * One `u32` at [`VINYL_AT`], holding [`crate::hash::string_hash`] of the applied vinyl's **menu
//!   name**. Locked the same way, on three vinyls applied in turn: each transition moved exactly
//!   twelve bytes, the digest included, and the third value was written down before the game wrote
//!   the file. It is the *menu* name — the pack calls it `240SX_FLAGS_SPAIN`, the profile stores
//!   the hash of `FLAGS_SPAIN` — and hashing all 1,773 of a car's menu-level names and searching
//!   the whole profile for any of them returns exactly this one offset.
//!
//! **What is not here:** the torque and power the game displays. The shop showed 3.64 and 1.75 for
//! one purchase and neither appears in the file as a float, at any scale. They are computed at run
//! time from the numbers above — which is why no torque table was ever found in any static asset:
//! there is not one.
//!
//! **One profile is not a format.** The offsets below were measured on a single save, so they are
//! checked rather than trusted: the magic, the length, and the shape of what is read (flags must be
//! 0 or 1, tuning values finite and small). A profile that fails those is refused, because handing
//! back plausible-looking numbers from the wrong offsets is worse than saying no.

use crate::error::{NfsError, NfsResult};
use crate::types::AssetHash;

/// `20CM`, the four bytes every profile starts with.
pub const MAGIC: &[u8; 4] = b"20CM";
/// The length every profile measured so far has had.
pub const LENGTH: usize = 54_966;

/// Start of the per-category tuning totals.
///
/// Exactly the three slots that were measured, and not one more. The first draft read fifteen on the
/// reasoning that the array must be contiguous — the neighbours turned out to hold values that are
/// not floats at all (`1.08e-36`, `1.07e+22` when read as such), so widening the window was a guess
/// and the real saves refused it. The array may well continue; nothing here has seen it.
pub const TUNING_AT: usize = 0x0BA4;
pub const TUNING_LEN: usize = 3;

/// Start of the per-product installed flags.
pub const INSTALLED_AT: usize = 0x0BF0;
/// How many flag bytes are read, for the same reason.
pub const INSTALLED_LEN: usize = 0x20;

/// The applied vinyl, as [`crate::hash::string_hash`] of its **menu name**.
///
/// Locked the same way the flag bytes were, by changing one thing and diffing. Three vinyls were
/// applied to the same car in turn, and each transition moved exactly twelve bytes: the digest at
/// `+0x14`, and this one `u32`.
///
/// | applied | value | is |
/// |---|---|---|
/// | Thailand | `0xDD8927B0` | `string_hash("FLAGS_THAILAND")` |
/// | Spain | `0x6C2AB886` | `string_hash("FLAGS_SPAIN")` |
/// | Netherlands | `0x8C066D23` | `string_hash("FLAGS_NETHERLANDS")` |
///
/// The third was **written down before the game wrote the file**, which is what makes this a result
/// rather than a coincidence — the same standard the differential swap was held to.
///
/// The name is the *menu* name and not the texture's: the pack calls it `240SX_FLAGS_THAILAND` and
/// the save stores the hash of `FLAGS_THAILAND`, with the car prefix gone. Hashing all 1,773
/// menu-level names in one car's `VINYLS.BIN` and searching the whole 54,966-byte profile for any of
/// them returns exactly one hit, at this offset — so the slot is not one of several places a vinyl
/// could be.
///
/// **One slot, and no claim about a second.** The 48 bytes after it are zero in every profile
/// measured, and the game lets a car carry more than one vinyl, so they are very likely further
/// layers — which is a guess, and [`Profile`] reads none of them. Widening a window because it
/// looked contiguous is the mistake [`TUNING_AT`] already records.
pub const VINYL_AT: usize = 0x0E28;

/// The categories whose slot is confirmed. Others are read but unnamed: a slot with no evidence
/// behind it gets an index, not a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Category {
    /// Gearbox, flywheel and differential — three products that coexist.
    Transmission,
    /// One slot; its products replace one another.
    Nitrous,
    Engine,
}

impl Category {
    /// Index into [`Profile::tuning`].
    #[must_use]
    pub fn slot(self) -> usize {
        // 0x0BA4, 0x0BA8, 0x0BAC — measured, in that order.
        match self {
            Self::Transmission => 0,
            Self::Nitrous => 1,
            Self::Engine => 2,
        }
    }
}

/// What a profile says about the car's performance parts.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Profile {
    /// Per-category totals, in file order. Index with [`Category::slot`].
    pub tuning: Vec<f32>,
    /// Per-product flags, in file order: `true` where a product is fitted.
    pub installed: Vec<bool>,
    /// The applied vinyl's menu-name hash, or `None` when the slot is zero — which is what a car
    /// with no vinyl on it reads. See [`VINYL_AT`].
    pub vinyl: Option<AssetHash>,
}

impl Profile {
    /// Read a profile.
    ///
    /// # Errors
    /// When the magic or the length is not a profile's, or when the two regions do not read as what
    /// they were measured to be — a flag outside `0..=1`, or a total that is not a small finite
    /// number. Those are the checks that stop this being applied to a file it does not describe.
    pub fn parse(bytes: &[u8]) -> NfsResult<Self> {
        if bytes.get(..4) != Some(MAGIC) {
            return Err(NfsError::BufferSizeMismatch { detail: "not a profile: bad magic" });
        }
        if bytes.len() != LENGTH {
            return Err(NfsError::BufferSizeMismatch { detail: "profile is not the measured length" });
        }

        let mut tuning = Vec::with_capacity(TUNING_LEN);
        for i in 0..TUNING_LEN {
            let at = TUNING_AT + i * 4;
            let b = bytes
                .get(at..at + 4)
                .ok_or(NfsError::BufferSizeMismatch { detail: "tuning past the end" })?;
            let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            // The measured values are 0..2. A slot holding something else means these are not the
            // bytes this reader was locked on.
            if !v.is_finite() || !(-1.0..=64.0).contains(&v) {
                return Err(NfsError::BufferSizeMismatch { detail: "tuning slot is not a total" });
            }
            tuning.push(v);
        }

        let flags = bytes
            .get(INSTALLED_AT..INSTALLED_AT + INSTALLED_LEN)
            .ok_or(NfsError::BufferSizeMismatch { detail: "flags past the end" })?;
        if flags.iter().any(|&b| b > 1) {
            return Err(NfsError::BufferSizeMismatch { detail: "flag byte is not 0 or 1" });
        }
        let v = bytes
            .get(VINYL_AT..VINYL_AT + 4)
            .ok_or(NfsError::BufferSizeMismatch { detail: "vinyl slot past the end" })?;
        let raw = u32::from_le_bytes([v[0], v[1], v[2], v[3]]);
        // Zero is "no vinyl", not a hash: it is what the slot held before one was ever applied.
        // Nothing else about the value is checked, because a hash has no shape to check against —
        // any u32 is a possible one, and inventing a range would refuse real saves.
        let vinyl = (raw != 0).then_some(AssetHash(raw));

        Ok(Self { tuning, installed: flags.iter().map(|&b| b == 1).collect(), vinyl })
    }

    /// The total for one category.
    #[must_use]
    pub fn total(&self, category: Category) -> f32 {
        self.tuning.get(category.slot()).copied().unwrap_or(0.0)
    }

    /// How many products are fitted across every category.
    #[must_use]
    pub fn fitted(&self) -> usize {
        self.installed.iter().filter(|&&b| b).count()
    }

    /// The applied vinyl's name, recovered by hashing the candidates a pack offers.
    ///
    /// The save stores only a hash, so a name comes back only if something here can produce it —
    /// which is the same trick [`crate::hash`] uses to recover a truncated texture name. Pass the
    /// texture names from the car's `VINYLS.BIN`; the car prefix is stripped before hashing, since
    /// the pack calls it `240SX_FLAGS_SPAIN` and the profile stores `FLAGS_SPAIN`.
    ///
    /// Returns `None` when no vinyl is applied, or when none of `candidates` hashes to what the
    /// file holds — a hash that nothing explains is left as a hash rather than guessed at.
    #[must_use]
    pub fn vinyl_name<'a>(
        &self,
        car: &str,
        candidates: impl IntoIterator<Item = &'a str>,
    ) -> Option<String> {
        let want = self.vinyl?;
        let prefix = format!("{car}_");
        for c in candidates {
            let menu = c.strip_prefix(prefix.as_str()).unwrap_or(c);
            if crate::hash::string_hash(menu) == want.0 {
                return Some(menu.to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile with the gearbox category at 0.99 and three of its products fitted — the state the
    /// real save was in after three purchases.
    fn synthetic() -> Vec<u8> {
        let mut b = vec![0_u8; LENGTH];
        b[..4].copy_from_slice(MAGIC);
        b[0x0BA4..0x0BA8].copy_from_slice(&0.99_f32.to_le_bytes());
        b[0x0BA8..0x0BAC].copy_from_slice(&2.0_f32.to_le_bytes());
        b[0x0BAC..0x0BB0].copy_from_slice(&0.51_f32.to_le_bytes());
        b[0x0BF6] = 1;
        b[0x0BF7] = 1;
        b[0x0BF8] = 1;
        b[0x0C00] = 1;
        b
    }

    #[test]
    fn the_measured_state_reads_back() {
        let p = Profile::parse(&synthetic()).expect("parses");
        assert!((p.total(Category::Transmission) - 0.99).abs() < 1e-6);
        assert!((p.total(Category::Nitrous) - 2.0).abs() < 1e-6);
        assert!((p.total(Category::Engine) - 0.51).abs() < 1e-6);
        assert_eq!(p.fitted(), 4);
    }

    /// The three vinyls that were applied in turn, each written back as the hash of its menu name.
    ///
    /// These are the measured values, not derived ones: the profile held exactly these three `u32`s
    /// after Thailand, Spain and Netherlands were applied to the same car, and the third was written
    /// down before the game saved. Asserting them *against `string_hash`* rather than as literals is
    /// the point — it is the same claim the save makes, and it fails if either side moves.
    #[test]
    fn the_applied_vinyl_is_its_menu_names_hash() {
        for (name, measured) in [
            ("FLAGS_THAILAND", 0xDD89_27B0u32),
            ("FLAGS_SPAIN", 0x6C2A_B886),
            ("FLAGS_NETHERLANDS", 0x8C06_6D23),
        ] {
            assert_eq!(crate::hash::string_hash(name), measured, "{name}");
            let mut b = synthetic();
            b[VINYL_AT..VINYL_AT + 4].copy_from_slice(&measured.to_le_bytes());
            let p = Profile::parse(&b).expect("parses");
            assert_eq!(p.vinyl, Some(AssetHash(measured)));
            // And the name comes back from a pack that offers it, car prefix and all.
            let pack = ["240SX_FLAGS_THAILAND", "240SX_FLAGS_SPAIN", "240SX_FLAGS_NETHERLANDS"];
            assert_eq!(p.vinyl_name("240SX", pack).as_deref(), Some(name));
        }
    }

    /// A car with no vinyl reads as none rather than as hash zero.
    #[test]
    fn an_unpainted_car_has_no_vinyl() {
        let p = Profile::parse(&synthetic()).expect("parses");
        assert_eq!(p.vinyl, None);
        assert_eq!(p.vinyl_name("240SX", ["240SX_FLAGS_SPAIN"]), None);
    }

    /// A hash nothing in the pack explains stays a hash. Guessing a name for it would put a label on
    /// the screen that the file never said.
    #[test]
    fn a_vinyl_no_candidate_explains_keeps_its_hash() {
        let mut b = synthetic();
        b[VINYL_AT..VINYL_AT + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let p = Profile::parse(&b).expect("parses");
        assert_eq!(p.vinyl, Some(AssetHash(0xDEAD_BEEF)));
        assert_eq!(p.vinyl_name("240SX", ["240SX_FLAGS_SPAIN"]), None);
    }

    /// The two products that replaced one another sit next to each other, so a swap is visible as a
    /// flag moving rather than a value changing.
    #[test]
    fn a_swap_moves_the_flag_to_its_neighbour() {
        let mut before = synthetic();
        before[0x0BFC] = 1;
        let mut after = before.clone();
        after[0x0BFC] = 0;
        after[0x0BFD] = 1;

        let (a, b) = (Profile::parse(&before).unwrap(), Profile::parse(&after).unwrap());
        assert_eq!(a.fitted(), b.fitted(), "a swap fits the same number of products");
        assert_ne!(a.installed, b.installed);
    }

    #[test]
    fn something_that_is_not_a_profile_is_refused() {
        assert!(Profile::parse(b"not a save").is_err());
        assert!(Profile::parse(&vec![0_u8; LENGTH]).is_err(), "right length, wrong magic");
    }

    /// The shape checks are what stop this reader being pointed at the wrong bytes.
    #[test]
    fn a_flag_that_is_not_a_flag_is_refused() {
        let mut b = synthetic();
        b[0x0BF6] = 7;
        assert!(Profile::parse(&b).is_err());
    }
}
