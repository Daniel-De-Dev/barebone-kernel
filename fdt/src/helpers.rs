//! A module for shared helpers

/// Converts a `u32` to `usize` without loss.
///
/// This conversion is infallible on all supported targets because the crate
/// requires `usize` to be at least 32 bits wide.
#[expect(
  clippy::as_conversions,
  reason = "the crate requires usize to be at least 32 bits, so every u32 fits in usize"
)]
pub(crate) const fn usize_from_u32(value: u32) -> usize {
  value as usize
}
