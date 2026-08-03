//! The city: `TRACKS/STREAM*.BUN`.
//!
//! Eight bundles, 324 MB, **28,985 objects** between them — `STREAML4RA` 10,735 and `STREAML4RD`
//! 11,135 alone. They are not adjacent tiles: they share one world coordinate system and overlap,
//! being per-race-route supersets of the same city.
//!
//! Two things about the container are worth knowing before reading any of it.
//!
//! **A bundle is not a contiguous chunk stream.** It is padded to sector boundaries for disc
//! streaming, and the gaps are not always whole `0x11` words — a walk that advances by
//! `8 + size` and stops at the first implausible header covers 4.5 % of `STREAML4RH.BUN` and finds
//! 13 of its 175 objects. The walk has to resync. [`crate::chunk::walk`] already tolerates this;
//! what a caller must not do is treat an early stop as the end of the file.
//!
//! **The container test is the high bit**, `id & 0x8000_0000`, not the top nibble. `0xB3300000` —
//! the texture pack every region carries — is a container that a top-nibble test walks straight
//! past. [`crate::chunk::CONTAINER_FLAG`] has this right; the note exists because a widely-cited
//! public description of this format does not.
//!
//! ## Status
//!
//! [`header`] reads the `0x00134011` solid header, which is the manifest layer: every object's
//! hash, extent and placement, with no vertex decoded. Geometry, the track texture-pack variant and
//! the route files are not here yet.

pub mod header;
pub mod manifest;

pub use header::{read_header, WorldSolidHeader};
pub use manifest::{manifest, WorldObjectInfo};
