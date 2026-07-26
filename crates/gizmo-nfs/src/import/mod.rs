//! Reading models back in — the inverse of [`crate::export`].
//!
//! The crate could write a car out and not read one back, which is the difference between an
//! exporter and a round trip. Nothing here knows about NFSU2: an [`obj::ObjMesh`] is what the text
//! file said, and turning one into a solid's buffers is [`crate::geometry::replace_mesh`]'s job.
//!
//! Two things are deliberately *not* done here, because a bare model file cannot know them:
//!
//! * **Which solid a mesh replaces.** 24.8% of the install's solids share their name with another
//!   solid in the same file, so a name is a hint and never an identity; a solid is named by its
//!   header offset.
//! * **The placement.** [`crate::export::obj`] bakes each solid's matrix into the positions it
//!   writes, so a re-import has to undo it — with the matrix from the solid, which the model file
//!   does not carry. See [`crate::placement`].

pub mod obj;
