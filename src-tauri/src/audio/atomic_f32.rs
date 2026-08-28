use std::sync::atomic::{AtomicU32, Ordering};

/// An `f32` shared with the audio thread. Rust has no atomic float, so the bits travel in
/// a `u32` and the conversion lives here instead of at every call site.
#[derive(Debug, Default)]
pub(crate) struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub(crate) fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    pub(crate) fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub(crate) fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    /// The value it replaced, so a caller can tell a real move from a repeated one.
    pub(crate) fn replace(&self, value: f32) -> f32 {
        f32::from_bits(self.0.swap(value.to_bits(), Ordering::Relaxed))
    }
}
