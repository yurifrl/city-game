//! Renders on the real GPU adapter. Fails (never skips) when none exists.

use city_render::{Box3, Camera, Renderer, Scene};

const W: u32 = 640;
const H: u32 = 360;

fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

fn scene<'a>(boxes: &'a [Box3], camera: Camera) -> Scene<'a> {
    Scene {
        grid: [96, 96],
        camera,
        boxes,
    }
}

#[test]
fn a_red_building_is_drawn_where_picking_says_its_cell_is() {
    let mut renderer = Renderer::new(W, H).expect("GPU adapter");
    let camera = Camera::new([40.5, 40.5], 12.0);
    let building = Box3 {
        min: [40.0, 40.0],
        size: [1.0, 1.0],
        base: 0.0,
        height: 1.5,
        color: [1.0, 0.0, 0.0],
    };

    let empty = renderer.render(&scene(&[], camera)).unwrap();
    let with = renderer.render(&scene(&[building], camera)).unwrap();
    assert_eq!(with.rgba.len(), (W * H * 4) as usize);

    // The building covers the screen centre (the camera looks at its cell).
    let centre = pixel(&with.rgba, W / 2, H / 2);
    assert!(centre[0] > 100 && centre[1] < 40 && centre[2] < 40, "centre pixel {centre:?} is not red");
    let ground = pixel(&empty.rgba, W / 2, H / 2);
    assert!(ground[1] > ground[0], "empty frame centre {ground:?} should be green ground");

    // Picking the centre pixel resolves to the building's cell, and a pixel
    // far from it does not.
    assert_eq!(camera.cell_at(W, H, W as f32 / 2.0, H as f32 / 2.0), Some([40, 40]));
    assert_ne!(camera.cell_at(W, H, 40.0, 40.0), Some([40, 40]));
}

#[test]
fn nearer_boxes_hide_farther_ones() {
    let mut renderer = Renderer::new(W, H).expect("GPU adapter");
    let camera = Camera::new([40.5, 40.5], 12.0);
    // Both boxes cover the screen centre; the taller one must win the pixel
    // regardless of draw order.
    let tall_blue = Box3 { min: [40.0, 40.0], size: [1.0, 1.0], base: 0.0, height: 4.0, color: [0.0, 0.0, 1.0] };
    let short_red = Box3 { min: [40.0, 40.0], size: [1.0, 1.0], base: 0.0, height: 1.0, color: [1.0, 0.0, 0.0] };
    for boxes in [[tall_blue, short_red], [short_red, tall_blue]] {
        let frame = renderer.render(&scene(&boxes, camera)).unwrap();
        let centre = pixel(&frame.rgba, W / 2, H / 2);
        assert!(centre[2] > centre[0], "taller blue box should be in front, got {centre:?}");
    }
}
