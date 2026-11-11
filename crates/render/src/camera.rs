//! Orthographic isometric camera and ground-plane picking.

use glam::{Mat4, Vec2, Vec3};

/// Smallest and largest `Camera::half_height`, in cells.
pub const MIN_HALF_HEIGHT: f32 = 4.0;
pub const MAX_HALF_HEIGHT: f32 = 80.0;

/// Isometric view looking at `center` (a point on the ground, in cells).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// (x, z) ground point at the image centre.
    pub center: [f32; 2],
    /// Half the visible ground height, in cells. Smaller = zoomed in.
    pub half_height: f32,
}

impl Camera {
    pub fn new(center: [f32; 2], half_height: f32) -> Self {
        Camera {
            center,
            half_height: half_height.clamp(MIN_HALF_HEIGHT, MAX_HALF_HEIGHT),
        }
    }

    /// Multiplies the zoom by `factor`, staying inside the allowed range.
    pub fn zoomed(self, factor: f32) -> Self {
        Camera::new(self.center, self.half_height * factor)
    }

    /// Shifts the camera by a screen-space drag of `(dx, dy)` pixels on an
    /// image `width` x `height` pixels, so the ground follows the pointer.
    pub fn panned(self, dx: f32, dy: f32, width: u32, height: u32) -> Self {
        let before = self.ground_at(width, height, 0.0, 0.0);
        let after = self.ground_at(width, height, dx, dy);
        match (before, after) {
            (Some(a), Some(b)) => Camera::new(
                [self.center[0] - (b.x - a.x), self.center[1] - (b.y - a.y)],
                self.half_height,
            ),
            // Only a degenerate, edge-on camera has no ground intersection;
            // the fixed isometric pitch never produces one.
            _ => self,
        }
    }

    pub(crate) fn view_proj(&self, width: u32, height: u32) -> Mat4 {
        let target = Vec3::new(self.center[0], 0.0, self.center[1]);
        // Classic isometric direction: 45 degrees around, ~35.26 degrees down.
        let eye = target + Vec3::new(1.0, 1.0, 1.0).normalize() * 200.0;
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let half_h = self.half_height;
        let half_w = half_h * width as f32 / height as f32;
        let proj = Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, 0.1, 500.0);
        proj * view
    }

    /// Ground (x, z) under pixel `(px, py)`, or `None` if the ray misses
    /// the ground plane.
    pub fn ground_at(&self, width: u32, height: u32, px: f32, py: f32) -> Option<Vec2> {
        let ndc_x = 2.0 * px / width as f32 - 1.0;
        let ndc_y = 1.0 - 2.0 * py / height as f32;
        let inverse = self.view_proj(width, height).inverse();
        let near = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        let dir = far - near;
        if dir.y.abs() < 1e-6 {
            return None;
        }
        let t = -near.y / dir.y;
        let hit = near + dir * t;
        Some(Vec2::new(hit.x, hit.z))
    }

    /// Grid cell under pixel `(px, py)`. The cell may lie outside the grid;
    /// bounds are the caller's concern.
    pub fn cell_at(&self, width: u32, height: u32, px: f32, py: f32) -> Option<[i32; 2]> {
        let ground = self.ground_at(width, height, px, py)?;
        Some([ground.x.floor() as i32, ground.y.floor() as i32])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_centre_picks_the_camera_centre_cell() {
        let camera = Camera::new([10.5, 20.5], 20.0);
        assert_eq!(camera.cell_at(1280, 720, 640.0, 360.0), Some([10, 20]));
    }

    #[test]
    fn zoom_is_clamped() {
        let camera = Camera::new([0.0, 0.0], 20.0);
        assert_eq!(camera.zoomed(1e-6).half_height, MIN_HALF_HEIGHT);
        assert_eq!(camera.zoomed(1e6).half_height, MAX_HALF_HEIGHT);
    }

    #[test]
    fn panning_keeps_the_grabbed_ground_point_under_the_pointer() {
        let start = Camera::new([30.0, 30.0], 20.0);
        let grabbed = start.ground_at(1280, 720, 400.0, 300.0).unwrap();
        let moved = start.panned(55.0, -35.0, 1280, 720);
        let under_pointer = moved.ground_at(1280, 720, 455.0, 265.0).unwrap();
        assert!((grabbed - under_pointer).length() < 1e-3);
    }
}
