//! Headless GPU renderer for the city. Draws a ground grid and instanced
//! boxes into an offscreen target and reads the pixels back for encoding.
//!
//! The crate knows nothing about buildings, players or the network: callers
//! describe a [`Scene`] of boxes in world units and get RGBA8 pixels.
//!
//! Conventions: world X/Z is the ground plane (one unit = one grid cell),
//! Y is up. [`Box3::min`] is the (x, z) corner with the smallest coordinates.
//! Pixel (0, 0) is the top-left of the image.

mod camera;
mod gpu;

pub use camera::Camera;
pub use gpu::Renderer;

/// One axis-aligned box standing on the ground plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box3 {
    /// (x, z) of the corner with the smallest coordinates, in cells.
    pub min: [f32; 2],
    /// Extent along (x, z), in cells.
    pub size: [f32; 2],
    /// Elevation of the bottom face above the ground, in cell units.
    pub base: f32,
    /// Box height from `base` upwards, in cell units.
    pub height: f32,
    /// Linear RGB in 0..=1.
    pub color: [f32; 3],
}

/// Everything needed to draw one frame.
#[derive(Debug, Clone)]
pub struct Scene<'a> {
    /// Ground grid extent in cells, anchored at the world origin.
    pub grid: [u32; 2],
    pub camera: Camera,
    pub boxes: &'a [Box3],
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter: {0}")]
    NoAdapter(String),
    #[error("GPU device request failed: {0}")]
    Device(String),
    #[error("frame readback failed: {0}")]
    Readback(String),
}

/// Pixels of one rendered frame, tightly packed RGBA8, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
