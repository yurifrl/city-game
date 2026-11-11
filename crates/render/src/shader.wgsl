struct Globals {
    view_proj: mat4x4<f32>,
    // x, y: grid extent in cells.
    grid: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexIn {
    @location(0) corner: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // x, z of the min corner; size along x, z.
    @location(2) rect: vec4<f32>,
    // x: bottom elevation; y: height; z: 1.0 for the ground slab, 0.0 for boxes.
    @location(3) params: vec3<f32>,
    @location(4) color: vec3<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) ground: f32,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let world = vec3<f32>(
        in.rect.x + in.corner.x * in.rect.z,
        in.params.x + in.corner.y * in.params.y,
        in.rect.y + in.corner.z * in.rect.w,
    );
    var out: VertexOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.normal = in.normal;
    out.color = in.color;
    out.ground = in.params.z;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let light = normalize(vec3<f32>(0.4, 1.0, 0.7));
    let diffuse = max(dot(normalize(in.normal), light), 0.0);
    var color = in.color * (0.45 + 0.55 * diffuse);
    if in.ground > 0.5 && in.normal.y > 0.5 {
        let edge = min(fract(in.world.x), fract(in.world.z));
        let edge_back = min(1.0 - fract(in.world.x), 1.0 - fract(in.world.z));
        if min(edge, edge_back) < 0.03 {
            color = color * 0.7;
        }
    }
    return vec4<f32>(color, 1.0);
}
