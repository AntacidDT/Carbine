use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use carbine_core::{
    DrawItem, Light, MeshData, MeshHandle, ParticleItem, ParticleShape, Scene, Sky,
};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

/// The world pass: one directional light over an optional skin, mixed toward the
/// colour of the air by how far each pixel stands from the eyes. A face that
/// points away from the sun keeps its colour at ambient brightness, which is what
/// makes a cube read as a solid.
///
/// Everything the light quality can switch off is switched off by a zero in a
/// uniform, and a zero leaves this exact arithmetic alone: the bare lambert term
/// `ambient + (1 - ambient) * facing` is what `low` draws, pixel for pixel, because
/// `shade` is one, the sky fill is none and there is no sheen to add.
const WORLD_SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    light_view: mat4x4<f32>,
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    color: vec4<f32>,
    sun: vec4<f32>,
    fog: vec4<f32>,
    eyes: vec4<f32>,
    casting: vec4<f32>,
    sheen: vec4<f32>,
    skinned: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var skin_sampler: sampler;
@group(0) @binding(2) var skin: texture_2d<f32>;
@group(0) @binding(3) var cast_map: texture_depth_2d;
@group(0) @binding(4) var cast_sampler: sampler_comparison;

struct Pixel {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world: vec3<f32>,
    @location(3) sunside: vec4<f32>,
};

@vertex
fn versus(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> Pixel {
    var pixel: Pixel;
    let place = uniforms.model * vec4<f32>(position, 1.0);
    pixel.clip = uniforms.view_proj * place;
    pixel.world = place.xyz;
    pixel.normal = normalize((uniforms.model * vec4<f32>(normal, 0.0)).xyz);
    pixel.uv = uv;
    pixel.sunside = uniforms.light_view * place;
    return pixel;
}

/// How much of the sun this pixel is standing in. The cast is the world seen from
/// the sun, so a pixel the sun's own picture cannot reach is behind something.
///
/// `casting.z` is how many readings the edge of a shade is smoothed with, and zero of
/// them is a rung that casts nothing: the arithmetic after it is then exactly the
/// unshadowed one.
fn shade(pixel: Pixel) -> f32 {
    let taps = i32(uniforms.casting.z);
    if (taps < 1) {
        return 1.0;
    }

    let on = pixel.sunside.xyz / pixel.sunside.w;
    let at = vec2<f32>(on.x * 0.5 + 0.5, on.y * 0.5 + 0.5);
    // Outside the cast the sun simply did not look this way, which is light.
    if (at.x < 0.0 || at.x > 1.0 || at.y < 0.0 || at.y > 1.0) {
        return 1.0;
    }

    // A pixel at the front of its own face would otherwise shade itself, so the
    // reading is asked for a little further back than the depth it landed on.
    let depth = on.z * 0.5 + 0.5 - uniforms.casting.w;
    var lit = 0.0;
    for (var i: i32 = 0; i < taps; i = i + 1) {
        // Taps laid on a golden-angle spiral, so a wider blur costs no clumping.
        let spread = uniforms.casting.x * uniforms.casting.y * sqrt((f32(i) + 0.5) / f32(taps));
        let turned = f32(i) * 2.3999632;
        let where_ = at + vec2<f32>(cos(turned), sin(turned)) * spread;
        lit = lit + textureSampleCompareLevel(cast_map, cast_sampler, where_, depth);
    }
    return lit / f32(taps);
}

@fragment
fn fragment(pixel: Pixel) -> @location(0) vec4<f32> {
    // Images are stored top row first and mesh v grows upward, so v is flipped.
    let skin = textureSample(skin, skin_sampler, vec2<f32>(pixel.uv.x, 1.0 - pixel.uv.y));
    let base = mix(uniforms.color.rgb, skin.rgb * uniforms.color.rgb, uniforms.skinned);
    let face = normalize(pixel.normal);
    let facing = max(dot(face, uniforms.sun.xyz), 0.0);
    let away = shade(pixel);
    let sunlit = (1.0 - uniforms.sun.w) * facing * away;
    // At `low` this stays the same scalar ambient it always was; from `medium` up
    // the light the sun leaves out comes from the backdrop over the face's head.
    let dome = mix(uniforms.horizon.rgb, uniforms.zenith.rgb, clamp(face.z, 0.0, 1.0));
    let ambient = mix(vec3<f32>(uniforms.sun.w), dome, uniforms.sheen.y);
    let lit = ambient + vec3<f32>(sunlit);
    // The sun coming back off a face that is squared at it, which is what a wet
    // or polished thing is. It is shut at `low` as well, and zero of it is nothing.
    let eye = normalize(uniforms.eyes.xyz - pixel.world);
    let turned = normalize(uniforms.sun.xyz + eye);
    let glossy = pow(max(dot(face, turned), 0.0), 24.0) * uniforms.sheen.x * away;
    // Fog is the air rather than anything falling through it, so it is a distance
    // rather than a shape: every metre between the pixel and the eyes takes some of
    // the picture away. A density of zero leaves the colour exactly as it was lit.
    let distance = length(pixel.world - uniforms.eyes.xyz);
    let veil = 1.0 - exp(-max(uniforms.fog.w, 0.0) * distance);
    let shaded = vec3<f32>(away, clamp(pixel.sunside.x / pixel.sunside.w * 0.5 + 0.5, 0.0, 1.0), clamp(pixel.sunside.y / pixel.sunside.w * 0.5 + 0.5, 0.0, 1.0));
    let tinted = mix(shaded, uniforms.fog.xyz, clamp(veil, 0.0, 1.0));
    return vec4<f32>(tinted, uniforms.color.a * mix(1.0, skin.a, uniforms.skinned));
}
"#;

/// The world seen from the sun, with nothing written but depth. This is the
/// picture the shade is read out of afterwards.
const CAST_SHADER: &str = r#"
struct CastUniforms {
    light_view: mat4x4<f32>,
    model: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: CastUniforms;

@vertex
fn versus(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> @builtin(position) vec4<f32> {
    return uniforms.light_view * uniforms.model * vec4<f32>(position, 1.0);
}
"#;

/// The backdrop: one triangle over the whole frame, shaded by unprojecting the
/// pixel back into a world direction, so the same maths serves both camera
/// modes.
const SKY_SHADER: &str = r#"
struct SkyUniforms {
    inverse_view_proj: mat4x4<f32>,
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    viewport: vec2<f32>,
    noise: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> uniforms: SkyUniforms;

@vertex
fn versus(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // (-1, -1), (3, -1), (-1, 3) covers the frame with a single triangle.
    let x = f32(index & 1u) * 4.0 - 1.0;
    let y = f32((index >> 1u) & 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn grain(point: vec2<f32>) -> f32 {
    let swept = sin(point.x * 12.9898 + point.y * 78.233) * 43758.5453;
    return fract(swept) - 0.5;
}

@fragment
fn fragment(@builtin(position) window: vec4<f32>) -> @location(0) vec4<f32> {
    let ndc = vec2<f32>(
        window.x / uniforms.viewport.x * 2.0 - 1.0,
        1.0 - window.y / uniforms.viewport.y * 2.0,
    );
    let near = uniforms.inverse_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let far = uniforms.inverse_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    let direction = normalize((far.xyz / far.w) - (near.xyz / near.w));

    // The world is z-up, so the dome opens along +z and anything at or below the
    // horizon keeps the horizon colour.
    let height = clamp(direction.z, 0.0, 1.0);
    let tint = mix(uniforms.horizon.rgb, uniforms.zenith.rgb, pow(height, 0.45));
    let dust = grain(window.xy) * uniforms.noise;
    return vec4<f32>(clamp(tint + vec3<f32>(dust), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

/// A crowd: one instance per thing in the air, each a quad stood up to face the
/// eyes at its own world position. Unlike the veil above, this is geometry, so a
/// wall hides the rain behind it and a drop lands on the floor it was thrown off.
///
/// The core says where each thing is, how wide it is, which way it travels, and
/// what colour it kept; how a streak, a chip and a mote each look is decided here.
const PARTICLE_SHADER: &str = r#"
struct ParticleUniforms {
    view_proj: mat4x4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    fog: vec4<f32>,
    eyes: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: ParticleUniforms;
@group(0) @binding(1) var skin_sampler: sampler;
@group(0) @binding(2) var skin: texture_2d<f32>;

struct Pixel {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) tint: vec4<f32>,
    @location(2) look: vec2<f32>,
    @location(3) world: vec3<f32>,
    @location(4) uv: vec2<f32>,
};

@vertex
fn versus(
    @location(0) corner: vec2<f32>,
    @location(1) place: vec4<f32>,
    @location(2) motion: vec4<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) look: vec4<f32>,
) -> Pixel {
    // The long axis of a streak is the way it travels, seen from the eyes: a drop
    // falling towards the camera stays a dot and one crossing the frame is a line.
    let speed = length(motion.xyz);
    let screen = vec2<f32>(
        dot(motion.xyz, uniforms.right.xyz),
        dot(motion.xyz, uniforms.up.xyz),
    );
    var long = uniforms.up.xyz;
    var wide = uniforms.right.xyz;
    if (length(screen) > 1.0e-4 * max(speed, 1.0)) {
        let seen = length(screen);
        long = uniforms.right.xyz * (screen.x / seen) + uniforms.up.xyz * (screen.y / seen);
        wide = uniforms.right.xyz * (-screen.y / seen) + uniforms.up.xyz * (screen.x / seen);
    }

    let half = max(place.w, 0.0) * 0.5;
    let stretch = max(motion.w, 1.0);
    let offset = long * (corner.x * half * stretch) + wide * (corner.y * half);

    var pixel: Pixel;
    pixel.clip = uniforms.view_proj * vec4<f32>(place.xyz + offset, 1.0);
    pixel.local = corner;
    pixel.tint = tint;
    pixel.look = look.xy;
    pixel.world = place.xyz;
    // The quad's own x axis is the way the drop travels and y is across it, while an
    // image is stored top row first and read left to right, so the skin's across axis
    // is corner.y and its top edge sits at corner.x.
    pixel.uv = vec2<f32>(corner.y * 0.5 + 0.5, 0.5 - corner.x * 0.5);
    return pixel;
}

/// Coverage of a shape whose signed distance is `distance`, measured in the
/// shortest half of the quad. `fwidth` is what makes one feather one pixel,
/// however far away or tiny the thing is.
fn coverage(distance: f32) -> f32 {
    let feather = max(fwidth(distance), 1.0e-5);
    return 1.0 - smoothstep(-feather, feather, distance);
}

@fragment
fn fragment(pixel: Pixel) -> @location(0) vec4<f32> {
    // 0 a streak, 1 a chip, 2 a mote. The first two are boxes in the quad's own
    // axes, and a streak is a capsule running the length of its long axis.
    var distance = 1.0;
    if (pixel.look.x < 0.5) {
        let stretch = max(pixel.look.y, 1.0);
        let along = vec2<f32>(pixel.local.x * stretch, pixel.local.y);
        let core = vec2<f32>(stretch - 1.0, 0.0);
        let nearest = vec2<f32>(clamp(along.x, -core.x, core.x), 0.0);
        distance = length(along - nearest) - 1.0;
    } else if (pixel.look.x < 1.5) {
        distance = max(abs(pixel.local.x), abs(pixel.local.y)) - 1.0;
    } else {
        distance = length(pixel.local) - 1.0;
    }

    let skin = textureSample(skin, skin_sampler, pixel.uv);
    let base = pixel.tint.rgb * skin.rgb;
    // Fog is the air the crowd falls through, so a far drop is as washed out as a
    // far wall.
    let away = length(pixel.world - uniforms.eyes.xyz);
    let haze = clamp(1.0 - exp(-max(uniforms.fog.w, 0.0) * away), 0.0, 1.0);
    let tinted = mix(base, uniforms.fog.xyz, haze);
    return vec4<f32>(tinted, pixel.tint.a * skin.a * coverage(distance));
}
"#;

/// UI is drawn as a signed distance field over a quad, so a corner value or a
/// side count costs nothing to change and every edge is antialiased. It is
/// measured in pixels and never tested against the world's depth, which is why
/// a widget is always on top.
const UI_SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    color: vec4<f32>,
    size: vec2<f32>,
    corner: f32,
    sides: f32,
    outline: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct Pixel {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
};

const PI: f32 = 3.14159265;

@vertex
fn versus(@location(0) position: vec3<f32>) -> Pixel {
    var pixel: Pixel;
    pixel.clip = uniforms.view_proj * uniforms.model * vec4<f32>(position, 1.0);
    pixel.local = vec2<f32>(position.x, position.y) * uniforms.size;
    return pixel;
}

/// Coverage of a shape whose signed distance is `distance`, in pixels, positive
/// outside. The field is in screen pixels, so a one pixel feather is exact.
fn coverage(distance: f32) -> f32 {
    return 1.0 - smoothstep(-0.5, 0.5, distance);
}

fn sd_round_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half + radius;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

/// Distance to a regular polygon, positive outside, from its inradius. The point
/// is folded into the sector facing +x, so one edge answers for all of them.
fn sd_polygon(p: vec2<f32>, inradius: f32, sides: f32) -> f32 {
    let step = 2.0 * PI / sides;
    let radial = length(p);
    let bearing = atan2(p.y, p.x);
    let angle = bearing - floor(bearing / step + 0.5) * step;
    let q = vec2<f32>(radial * cos(angle), radial * sin(angle));
    let half_edge = inradius * tan(0.5 * step);
    let nearest = vec2<f32>(inradius, clamp(q.y, -half_edge, half_edge));
    let distance = length(q - nearest);
    return select(-distance, distance, q.x >= inradius);
}

@fragment
fn fragment(pixel: Pixel) -> @location(0) vec4<f32> {
    let half = max(uniforms.size * 0.5, vec2<f32>(0.0));
    let radius = max(uniforms.corner, 0.0);
    let sides = max(uniforms.sides, 3.0);

    var distance: f32;
    if (abs(sides - 4.0) < 0.5) {
        // A rectangle uses both axes; any other count is a regular polygon
        // whose points touch the shorter axis of the box.
        distance = sd_round_box(pixel.local, half, min(radius, min(half.x, half.y)));
    } else {
        let flat = min(half.x, half.y) * cos(PI / sides);
        let rounding = min(radius, flat);
        distance = sd_polygon(pixel.local, flat - rounding, sides) - rounding;
    }

    let fill = coverage(distance);
    let band = fill - coverage(distance + max(uniforms.outline, 0.0));
    return vec4<f32>(uniforms.color.rgb, max(fill * uniforms.color.a, band));
}
"#;

/// Carbine black, the editor viewport background.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.086,
    g: 0.082,
    b: 0.078,
    a: 1.0,
};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const COPY_ROW_ALIGNMENT: u32 = 256;

/// How wide the cast is when the rung does not cast at all. The world shader never
/// reads a texel of it, but its layout still asks for a depth texture to be bound,
/// and one is the cheapest answer.
const BARE_CAST: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    /// The same world from the sun's side, which is what a shade is read out of.
    light_view: [[f32; 4]; 4],
    zenith: [f32; 4],
    horizon: [f32; 4],
    color: [f32; 4],
    sun: [f32; 4],
    fog: [f32; 4],
    eyes: [f32; 4],
    /// The cast: one texel in the picture, how far a soft edge reaches in texels,
    /// how many readings to take, and how far back the depth is read from. All
    /// zeroes is a rung that casts nothing. (`cast` is a reserved word in WGSL.)
    casting: [f32; 4],
    /// How much of the sun a face throws back, and how much of the light the sun
    /// leaves out comes from the backdrop. Both zero is the bare lambert world.
    sheen: [f32; 4],
    skinned: f32,
    _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CastUniforms {
    light_view: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SkyUniforms {
    inverse_view_proj: [[f32; 4]; 4],
    zenith: [f32; 4],
    horizon: [f32; 4],
    viewport: [f32; 2],
    noise: f32,
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct UiUniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    color: [f32; 4],
    size: [f32; 2],
    corner: f32,
    sides: f32,
    outline: f32,
    _pad: [f32; 3],
}

/// What a crowd is drawn with: the same camera for every thing in it, plus the air
/// its drops are washed into.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleUniforms {
    view_proj: [[f32; 4]; 4],
    /// The world direction that is screen right, taken from the eyes' own view.
    right: [f32; 4],
    /// And screen up. A streak is turned to face the lens out of these two.
    up: [f32; 4],
    fog: [f32; 4],
    eyes: [f32; 4],
}

/// One thing in the air, as the crowd's instance buffer holds it.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleInstance {
    /// Where it stands, in meters, and how wide it is.
    place: [f32; 4],
    /// How fast and which way it travels, and how many widths a streak is long.
    motion: [f32; 4],
    tint: [f32; 4],
    /// Which shape to cut, how long the streak is, and how far the thing stands
    /// from the eyes - back to front is the order they are drawn in.
    look: [f32; 4],
}

impl ParticleInstance {
    fn of(item: &ParticleItem<'_>, eyes: Vec3) -> ParticleInstance {
        let code = match item.shape {
            ParticleShape::Streak => 0.0,
            ParticleShape::Square => 1.0,
            ParticleShape::Circle => 2.0,
        };
        let stretch = item.shape.stretch();
        ParticleInstance {
            place: [item.at.x, item.at.y, item.at.z, item.size],
            motion: [item.velocity.x, item.velocity.y, item.velocity.z, stretch],
            tint: [item.color[0], item.color[1], item.color[2], item.alpha],
            look: [code, stretch, (item.at - eyes).length(), 0.0],
        }
    }
}

/// How a pipeline draws: which shader, whether it is tested against depth, and
/// whether it binds a skin.
struct Spec {
    label: &'static str,
    source: &'static str,
    uniform_bytes: usize,
    /// A pipeline with no vertex buffer at all, used by the backdrop.
    fullscreen: bool,
    /// A pipeline whose second vertex buffer is one row per thing drawn, used by a
    /// crowd.
    instanced: bool,
    skinned: bool,
    /// Whether the pass it draws into carries a depth attachment.
    depth: bool,
    depth_write: bool,
    /// A pipeline with no fragment stage at all: it leaves nothing on the screen
    /// and only says where the world's surface is, which is all a cast needs.
    depth_only: bool,
}

const WORLD: Spec = Spec {
    label: "carbine world",
    source: WORLD_SHADER,
    uniform_bytes: std::mem::size_of::<Uniforms>(),
    fullscreen: false,
    instanced: false,
    skinned: true,
    depth: true,
    depth_write: true,
    depth_only: false,
};

/// Translucent objects blend over what is already in the frame, so they must not
/// claim the depth they land on.
const GLASS: Spec = Spec {
    label: "carbine glass",
    source: WORLD_SHADER,
    uniform_bytes: std::mem::size_of::<Uniforms>(),
    fullscreen: false,
    instanced: false,
    skinned: true,
    depth: true,
    depth_write: false,
    depth_only: false,
};

/// The world seen from the sun. Nothing is drawn in colour here; the only thing
/// this pass leaves behind is the depth a shade is read out of.
const CAST: Spec = Spec {
    label: "carbine cast",
    source: CAST_SHADER,
    uniform_bytes: std::mem::size_of::<CastUniforms>(),
    fullscreen: false,
    instanced: false,
    skinned: false,
    depth: true,
    depth_write: true,
    depth_only: true,
};

const SKY: Spec = Spec {
    label: "carbine sky",
    source: SKY_SHADER,
    uniform_bytes: std::mem::size_of::<SkyUniforms>(),
    fullscreen: true,
    instanced: false,
    skinned: false,
    depth: true,
    depth_write: false,
    depth_only: false,
};

const UI: Spec = Spec {
    label: "carbine ui",
    source: UI_SHADER,
    uniform_bytes: std::mem::size_of::<UiUniforms>(),
    fullscreen: false,
    instanced: false,
    skinned: false,
    depth: false,
    depth_write: false,
    depth_only: false,
};

/// A crowd of drops: blended over the world it falls through, tested against its
/// depth so a wall hides what is behind it, and never claiming that depth itself.
const PARTICLES: Spec = Spec {
    label: "carbine particles",
    source: PARTICLE_SHADER,
    uniform_bytes: std::mem::size_of::<ParticleUniforms>(),
    fullscreen: false,
    instanced: true,
    skinned: true,
    depth: true,
    depth_write: false,
    depth_only: false,
};

#[derive(Debug, Clone)]
pub enum RenderError {
    NoAdapter(String),
    Device(String),
    Surface(String),
    Window(String),
    Readback(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(detail) => write!(
                formatter,
                "no Vulkan adapter was available ({detail}); install a Vulkan driver"
            ),
            Self::Device(detail) => {
                write!(formatter, "the GPU refused to create a device: {detail}")
            }
            Self::Surface(detail) => write!(formatter, "the surface could not be used: {detail}"),
            Self::Window(detail) => write!(formatter, "no window is available: {detail}"),
            Self::Readback(detail) => write!(formatter, "pixel readback failed: {detail}"),
        }
    }
}

impl std::error::Error for RenderError {}

pub fn create_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

struct GpuMesh {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    index_count: u32,
}

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform_bytes: u64,
    /// One uniform buffer per draw slot, grown as the scene needs more. Reusing a
    /// single buffer for a whole frame would make every object read the last
    /// upload instead of its own.
    uniforms: Vec<wgpu::Buffer>,
    skinned: bool,
}

/// A GPU device plus the caches that keep a frame cheap: one vertex and index
/// buffer per mesh, one texture per image path, one pipeline per surface format.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    /// What kind of device this is and how much memory it owns, as far as the
    /// machine will say. The core sorts a light rung out of this.
    pub hardware: carbine_core::Gpu,
    sampler: wgpu::Sampler,
    meshes: HashMap<MeshHandle, GpuMesh>,
    /// `None` records a skin that could not be read, so a broken file is opened
    /// once rather than once per frame.
    skins: HashMap<String, Option<wgpu::TextureView>>,
    /// The white pixel every untextured object is skinned with.
    plain: wgpu::TextureView,
    world: HashMap<wgpu::TextureFormat, Pipeline>,
    glass: HashMap<wgpu::TextureFormat, Pipeline>,
    casts: HashMap<wgpu::TextureFormat, Pipeline>,
    sky: HashMap<wgpu::TextureFormat, Pipeline>,
    particles: HashMap<wgpu::TextureFormat, Pipeline>,
    ui: HashMap<wgpu::TextureFormat, Pipeline>,
    /// The depth the sun sees: the world drawn from its side, at `0` texels on a
    /// side. A rung that casts nothing is held at one texel wide, so the map the
    /// rung above it needed is let go.
    cast: (u32, wgpu::Texture, wgpu::TextureView),
    /// Reads a pixel's own place in the cast and answers whether the sun reaches it.
    cast_sampler: wgpu::Sampler,
    /// The one quad every instanced drop is cut from, in its own axes.
    quad: GpuMesh,
    /// The instance buffer the crowd is uploaded into, kept and grown between
    /// frames so a steady rain never allocates again.
    crowd: Option<wgpu::Buffer>,
}

impl Gpu {
    pub fn new(
        instance: wgpu::Instance,
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface,
            ..Default::default()
        }))
        .map_err(|error| RenderError::NoAdapter(error.to_string()))?;

        let name = {
            let info = adapter.get_info();
            format!("{} ({:?})", info.name, info.backend)
        };
        // Skins are BC7 blocks straight from the file, so a GPU that cannot read them
        // cannot show a Carbine scene at all. Saying so here beats a device error from
        // the first texture write.
        if !adapter
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
        {
            return Err(RenderError::Device(format!(
                "{name} cannot decompress BC textures, which every packed Carbine skin needs"
            )));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("carbine"),
            required_features: wgpu::Features::TEXTURE_COMPRESSION_BC,
            ..Default::default()
        }))
        .map_err(|error| RenderError::Device(error.to_string()))?;

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("carbine skin sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let plain = upload_rgba(&device, &queue, "carbine plain skin", 1, 1, &[255; 4]);
        let quad = unit_quad(&device);
        let cast_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("carbine cast sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            // A pixel is standing in the sun when it is no further from the sun
            // than the surface the cast found where it is.
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let cast = make_cast(&device, BARE_CAST);
        let hardware = crate::device::describe(&adapter);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            adapter_name: name,
            hardware,
            sampler,
            meshes: HashMap::new(),
            skins: HashMap::new(),
            plain,
            world: HashMap::new(),
            glass: HashMap::new(),
            casts: HashMap::new(),
            sky: HashMap::new(),
            particles: HashMap::new(),
            ui: HashMap::new(),
            cast,
            cast_sampler,
            quad,
            crowd: None,
        })
    }

    pub fn adapter(&self) -> &str {
        &self.adapter_name
    }

    /// The device this renderer drew on, as the core wants it.
    pub fn hardware(&self) -> carbine_core::Gpu {
        self.hardware
    }

    /// A surface format that passes our sRGB colours straight through.
    pub fn pick_format(
        &self,
        surface: &wgpu::Surface<'_>,
    ) -> Result<wgpu::TextureFormat, RenderError> {
        let capabilities = surface.get_capabilities(&self.adapter);
        let plain = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ];
        capabilities
            .formats
            .iter()
            .find(|format| plain.contains(format))
            .or_else(|| capabilities.formats.first())
            .copied()
            .ok_or_else(|| RenderError::Surface("the window supports no color format".into()))
    }

    /// How the swap chain is allowed to run. Synced means the screen decides, so
    /// every frame waits for its refresh. Unlocked prefers the queued mailbox,
    /// which draws as fast as it can without tearing, and takes immediate when the
    /// compositor will not queue; a desktop that offers neither keeps waiting,
    /// because a tear is worse than a rate the machine never had anyway.
    pub fn pick_present_mode(&self, surface: &wgpu::Surface<'_>, sync: bool) -> wgpu::PresentMode {
        if sync {
            return wgpu::PresentMode::Fifo;
        }
        let offered = surface.get_capabilities(&self.adapter).present_modes;
        [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
            .iter()
            .find(|mode| offered.contains(mode))
            .copied()
            .unwrap_or(wgpu::PresentMode::Fifo)
    }

    pub fn configure_surface(
        &self,
        surface: &wgpu::Surface<'_>,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        present_mode: wgpu::PresentMode,
    ) -> wgpu::SurfaceConfiguration {
        let mut config = surface
            .get_default_config(&self.adapter, width.max(1), height.max(1))
            .expect("the surface lost its configuration options");
        config.format = format;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.present_mode = present_mode;
        surface.configure(&self.device, &config);
        config
    }

    /// An offscreen RGBA8 target, drawn into and then copied back to the CPU.
    pub fn create_target(&self, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("carbine target"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// The depth buffer that lets the world pass decide what is in front. The UI
    /// overlay never reads or writes it, which is why widgets stay on top.
    pub fn create_depth(&self, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("carbine depth"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// The cast at the size the light's rung asks for. Moving a rung up or down is
    /// the only time this costs anything, and moving down hands the bigger map back.
    fn ensure_cast(&mut self, size: u32) {
        if self.cast.0 == size {
            return;
        }
        self.cast = make_cast(&self.device, size);
    }

    /// Records the scene and submits it, returning the submission index so the
    /// caller can present the texture it rendered into. `extent` is the target in
    /// pixels: the world is measured against its aspect ratio, UI against its size.
    pub fn draw(
        &mut self,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        extent: (u32, u32),
        scene: &Scene,
    ) -> Result<wgpu::SubmissionIndex, RenderError> {
        let camera = scene.camera();
        let sky = scene.sky();
        let haze = scene.haze();
        let light = scene.light();
        let aspect = extent.0 as f32 / extent.1.max(1) as f32;
        let view_proj = camera.view_projection(aspect);

        // The sun's own picture of the world is taken before anything is drawn into
        // the frame that reads it. A rung that casts nothing keeps its bindings
        // pointed at a texel wide map and a shader that never looks at it.
        let casting = light.shadows;
        let light_view = if casting {
            sun_view(sky.sun, camera.position, light.reach)
        } else {
            Mat4::IDENTITY
        };
        self.ensure_cast(if casting { light.shadow_map } else { BARE_CAST });
        let cast = self.cast.2.clone();

        // Solid objects first, then the translucent ones back to front, so each
        // blends over what the camera sees behind it.
        let mut opaque: Vec<DrawItem<'_>> = Vec::new();
        let mut glass: Vec<(f32, DrawItem<'_>)> = Vec::new();
        for item in scene.draw_items() {
            if item.alpha >= 1.0 {
                opaque.push(item);
            } else {
                glass.push((
                    (item.model.transform_point3(Vec3::ZERO) - camera.position).length(),
                    item,
                ));
            }
        }
        glass.sort_by(|(this, _), (other, _)| other.total_cmp(this));

        let skins: Vec<wgpu::TextureView> = opaque
            .iter()
            .map(|item| self.skin_for(item.texture))
            .collect();
        let glass_skins: Vec<wgpu::TextureView> = glass
            .iter()
            .map(|(_, item)| self.skin_for(item.texture))
            .collect();
        let ui_items = scene.ui_items();

        // A crowd is one instanced draw per skin, so the drops are put in image
        // order first and then back to front inside each run.
        let items = scene.particle_items();
        let mut crowd: Vec<(usize, f32)> = items
            .iter()
            .enumerate()
            .map(|(index, item)| (index, (item.at - camera.position).length()))
            .collect();
        crowd.sort_by(|(this, this_away), (other, other_away)| {
            items[*this]
                .texture
                .cmp(&items[*other].texture)
                .then(other_away.total_cmp(this_away))
        });
        let mut instances: Vec<ParticleInstance> = Vec::with_capacity(crowd.len());
        let mut runs: Vec<(Option<String>, wgpu::TextureView, u32, u32)> = Vec::new();
        for (index, _) in crowd {
            let item = &items[index];
            instances.push(ParticleInstance::of(item, camera.position));
            let skin_key = item.texture.map(str::to_owned);
            match runs.last_mut().filter(|run| run.0 == skin_key) {
                Some(run) => run.3 += 1,
                None => {
                    let skin = self.skin_for(item.texture);
                    runs.push((skin_key, skin, instances.len() as u32 - 1, 1));
                }
            }
        }

        let world = ensure_pipeline(&mut self.world, &self.device, format, &WORLD, opaque.len());
        let glass_pipeline =
            ensure_pipeline(&mut self.glass, &self.device, format, &GLASS, glass.len());
        let sky_pipeline = if sky.enabled {
            Some(ensure_pipeline(
                &mut self.sky,
                &self.device,
                format,
                &SKY,
                1,
            ))
        } else {
            None
        };
        let particles_pipeline = if runs.is_empty() {
            None
        } else {
            // One set of uniforms serves the whole crowd: it is the same camera and
            // the same air, drop against drop.
            Some(ensure_pipeline(
                &mut self.particles,
                &self.device,
                format,
                &PARTICLES,
                1,
            ))
        };
        let ui_pipeline = if ui_items.is_empty() {
            None
        } else {
            Some(ensure_pipeline(
                &mut self.ui,
                &self.device,
                format,
                &UI,
                ui_items.len(),
            ))
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("carbine frame"),
            });

        // The sun takes its picture of the world before anything is drawn into the
        // frame that reads it, so a shade is always this frame's and never the one
        // before it.
        if casting {
            let pipeline =
                ensure_pipeline(&mut self.casts, &self.device, format, &CAST, opaque.len());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("carbine cast"),
                // Only depth is written: the cast is a picture of where the surface
                // of the world is, not of what colour it is.
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &cast,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline.pipeline);
            for (index, item) in opaque.iter().enumerate() {
                let mesh = ensure_mesh(&mut self.meshes, &self.device, item.handle, item.mesh);
                let uniforms = CastUniforms {
                    light_view: light_view.to_cols_array_2d(),
                    model: item.model.to_cols_array_2d(),
                };
                self.queue.write_buffer(
                    &pipeline.uniforms[index],
                    0,
                    bytemuck::bytes_of(&uniforms),
                );
                let bind = bind_group(
                    &self.device,
                    &pipeline.layout,
                    &pipeline.uniforms[index],
                    &self.sampler,
                    &self.plain,
                    pipeline.skinned,
                    None,
                );
                pass.set_bind_group(0, &bind, &[]);
                pass.set_vertex_buffer(0, mesh.vertex.slice(..));
                pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("carbine frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some(pipeline) = sky_pipeline {
                let uniforms = sky_uniforms(&view_proj, &sky, extent);
                self.queue
                    .write_buffer(&pipeline.uniforms[0], 0, bytemuck::bytes_of(&uniforms));
                let bind = bind_group(
                    &self.device,
                    &pipeline.layout,
                    &pipeline.uniforms[0],
                    &self.sampler,
                    &self.plain,
                    pipeline.skinned,
                    None,
                );
                pass.set_pipeline(&pipeline.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }

            for (index, item) in opaque.iter().enumerate() {
                let mesh = ensure_mesh(&mut self.meshes, &self.device, item.handle, item.mesh);
                let uniforms = world_uniforms(
                    &view_proj,
                    item,
                    &sky,
                    &haze,
                    camera.position,
                    &light,
                    &light_view,
                );
                self.queue
                    .write_buffer(&world.uniforms[index], 0, bytemuck::bytes_of(&uniforms));
                let bind = bind_group(
                    &self.device,
                    &world.layout,
                    &world.uniforms[index],
                    &self.sampler,
                    &skins[index],
                    world.skinned,
                    Some((&cast, &self.cast_sampler)),
                );
                pass.set_pipeline(&world.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.set_vertex_buffer(0, mesh.vertex.slice(..));
                pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }

            for (index, (_, item)) in glass.iter().enumerate() {
                let mesh = ensure_mesh(&mut self.meshes, &self.device, item.handle, item.mesh);
                let uniforms = world_uniforms(
                    &view_proj,
                    item,
                    &sky,
                    &haze,
                    camera.position,
                    &light,
                    &light_view,
                );
                self.queue.write_buffer(
                    &glass_pipeline.uniforms[index],
                    0,
                    bytemuck::bytes_of(&uniforms),
                );
                let bind = bind_group(
                    &self.device,
                    &glass_pipeline.layout,
                    &glass_pipeline.uniforms[index],
                    &self.sampler,
                    &glass_skins[index],
                    glass_pipeline.skinned,
                    Some((&cast, &self.cast_sampler)),
                );
                pass.set_pipeline(&glass_pipeline.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.set_vertex_buffer(0, mesh.vertex.slice(..));
                pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }

            // The crowd is real geometry, so it is tested against the depth the
            // world wrote: a wall hides the rain behind it and a floor is where it
            // lands. It goes over the glass it falls past and under the interface.
            if let Some(pipeline) = particles_pipeline {
                let view_matrix = camera.view();
                let right = view_matrix.row(0).truncate();
                let up = view_matrix.row(1).truncate();
                let uniforms = ParticleUniforms {
                    view_proj: view_proj.to_cols_array_2d(),
                    right: [right.x, right.y, right.z, 0.0],
                    up: [up.x, up.y, up.z, 0.0],
                    fog: haze,
                    eyes: [camera.position.x, camera.position.y, camera.position.z, 0.0],
                };
                self.queue
                    .write_buffer(&pipeline.uniforms[0], 0, bytemuck::bytes_of(&uniforms));
                let buffer = ensure_crowd(
                    &mut self.crowd,
                    &self.device,
                    &self.queue,
                    bytemuck::cast_slice(&instances),
                );
                let crowd_bytes =
                    (instances.len() * std::mem::size_of::<ParticleInstance>()) as u64;

                pass.set_pipeline(&pipeline.pipeline);
                pass.set_vertex_buffer(0, self.quad.vertex.slice(..));
                pass.set_vertex_buffer(1, buffer.slice(0..crowd_bytes));
                pass.set_index_buffer(self.quad.index.slice(..), wgpu::IndexFormat::Uint32);
                for (_, skin, first, count) in &runs {
                    let bind = bind_group(
                        &self.device,
                        &pipeline.layout,
                        &pipeline.uniforms[0],
                        &self.sampler,
                        skin,
                        pipeline.skinned,
                        None,
                    );
                    pass.set_bind_group(0, &bind, &[]);
                    pass.draw_indexed(0..self.quad.index_count, 0, *first..*first + *count);
                }
            }
        }

        if let Some(pipeline) = ui_pipeline {
            let ui_proj = scene.ui_projection(extent.0, extent.1);

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("carbine ui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline.pipeline);

                for (index, item) in ui_items.iter().enumerate() {
                    let mesh = ensure_mesh(&mut self.meshes, &self.device, item.handle, item.mesh);
                    let uniforms = UiUniforms {
                        view_proj: ui_proj.to_cols_array_2d(),
                        model: item.model.to_cols_array_2d(),
                        color: item.color,
                        size: item.size,
                        corner: item.corner,
                        sides: item.sides,
                        outline: item.outline,
                        _pad: [0.0; 3],
                    };
                    self.queue.write_buffer(
                        &pipeline.uniforms[index],
                        0,
                        bytemuck::bytes_of(&uniforms),
                    );
                    let bind = bind_group(
                        &self.device,
                        &pipeline.layout,
                        &pipeline.uniforms[index],
                        &self.sampler,
                        &self.plain,
                        pipeline.skinned,
                        None,
                    );
                    pass.set_bind_group(0, &bind, &[]);
                    pass.set_vertex_buffer(0, mesh.vertex.slice(..));
                    pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }
        }

        Ok(self.queue.submit([encoder.finish()]))
    }

    /// The skin for a path, reading and uploading it the first time it is asked
    /// for. A file that is not a packed image leaves the object in its own colour;
    /// the core is what tells the script about it.
    fn skin_for(&mut self, path: Option<&str>) -> wgpu::TextureView {
        let Some(path) = path else {
            return self.plain.clone();
        };
        if !self.skins.contains_key(path) {
            let skin = std::fs::read(path).ok().and_then(|bytes| {
                let texture = carbine_ktx::read(&bytes).ok()?;
                Some(upload_packed(
                    &self.device,
                    &self.queue,
                    "carbine skin",
                    &texture,
                ))
            });
            self.skins.insert(path.to_owned(), skin);
        }
        match &self.skins[path] {
            Some(view) => view.clone(),
            None => self.plain.clone(),
        }
    }

    pub fn present(&self, texture: wgpu::SurfaceTexture) {
        self.queue.present(texture);
    }

    /// Copies a render target into CPU memory as packed RGBA8, row by row.
    pub fn read_pixels(
        &mut self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RenderError> {
        let bytes_per_row = (width * 4).next_multiple_of(COPY_ROW_ALIGNMENT);
        let size = u64::from(bytes_per_row) * u64::from(height);
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("carbine readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("carbine readback"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let index = self.queue.submit([encoder.finish()]);

        let view = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        view.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: None,
            })
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        receiver
            .recv()
            .map_err(|_| RenderError::Readback("the map callback was dropped".into()))?
            .map_err(|error| RenderError::Readback(error.to_string()))?;

        let padded = view
            .get_mapped_range()
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        let mut packed = vec![0u8; (width * height * 4) as usize];
        let run = (width * 4) as usize;
        for row in 0..height {
            let start = (row * bytes_per_row) as usize;
            let target = (row * width * 4) as usize..((row + 1) * width * 4) as usize;
            packed[target].copy_from_slice(&padded[start..start + run]);
        }
        drop(padded);
        staging.unmap();

        Ok(packed)
    }
}

fn sky_uniforms(view_proj: &Mat4, sky: &Sky, extent: (u32, u32)) -> SkyUniforms {
    SkyUniforms {
        inverse_view_proj: view_proj.inverse().to_cols_array_2d(),
        zenith: [sky.zenith.x, sky.zenith.y, sky.zenith.z, 1.0],
        horizon: [sky.horizon.x, sky.horizon.y, sky.horizon.z, 1.0],
        viewport: [extent.0.max(1) as f32, extent.1.max(1) as f32],
        noise: sky.noise,
        _pad: 0.0,
    }
}

/// A square depth texture the sun's own picture of the world is drawn into.
fn make_cast(device: &wgpu::Device, size: u32) -> (u32, wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("carbine cast"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (size, texture, view)
}

/// The world seen from the sun, squared onto the cast.
///
/// The picture is centred on the eyes, because a shade is only worth drawing where
/// someone is standing to see it: everything past the reach stays lit and casts
/// nothing, which is how a wide reach stays cheap. The eyes sit `reach` in from the
/// front of the box, so the whole thing falls between the near and far planes.
fn sun_view(sun: Vec3, eyes: Vec3, reach: f32) -> Mat4 {
    // A sun almost overhead has no world +z left to stand the picture up with.
    let up = if sun.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
    let seen_from = eyes + sun * reach;
    let box_of_view = glam::camera::rh::proj::directx::orthographic(
        -reach,
        reach,
        -reach,
        reach,
        0.1,
        2.0 * reach,
    );
    box_of_view * glam::camera::rh::view::look_at_mat4(seen_from, eyes, up)
}

#[expect(
    clippy::too_many_arguments,
    reason = "one uniform block, everything the light pass reads"
)]
fn world_uniforms(
    view_proj: &Mat4,
    item: &DrawItem,
    sky: &Sky,
    haze: &[f32; 4],
    eyes: Vec3,
    light: &Light,
    light_view: &Mat4,
) -> Uniforms {
    let map = light.shadow_map.max(1) as f32;
    let taps = light.softness as f32;
    Uniforms {
        view_proj: view_proj.to_cols_array_2d(),
        model: item.model.to_cols_array_2d(),
        light_view: light_view.to_cols_array_2d(),
        zenith: [sky.zenith.x, sky.zenith.y, sky.zenith.z, 0.0],
        horizon: [sky.horizon.x, sky.horizon.y, sky.horizon.z, 0.0],
        color: [item.color[0], item.color[1], item.color[2], item.alpha],
        sun: [sky.sun.x, sky.sun.y, sky.sun.z, sky.ambient],
        fog: *haze,
        eyes: [eyes.x, eyes.y, eyes.z, 0.0],
        // A rung that casts nothing leaves the whole of this read as "no lookup":
        // no texel to spread over, no taps to take, and a depth never compared.
        //
        // The last is how far back the cast is read from, so a pixel does not shade
        // the face it stands on. Two texels of a map that holds the whole reach in
        // one unit of depth is two steps of it.
        casting: [1.0 / map, taps, taps, 2.0 / map],
        // The backdrop only lends light when there is a backdrop drawn to lend it.
        sheen: [
            light.gloss,
            f32::from(light.sky_fill && sky.enabled),
            0.0,
            0.0,
        ],
        skinned: f32::from(item.texture.is_some()),
        _pad: [0.0; 3],
    }
}

/// Pipelines whose shader reads a skin also need the sampler and texture bound
/// beside the uniforms; a pipeline whose layout was deduced from a shader that
/// reads neither must not be given them.
///
/// The same goes for the cast, which only the world and glass shaders declare. It
/// is filled on a rung that casts nothing too, because a deduced layout asks for
/// every binding the shader mentions whether or not this frame reads it.
#[expect(
    clippy::too_many_arguments,
    reason = "one bind group, every binding it can hold"
)]
fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    skin: &wgpu::TextureView,
    skinned: bool,
    cast: Option<(&wgpu::TextureView, &wgpu::Sampler)>,
) -> wgpu::BindGroup {
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: uniforms.as_entire_binding(),
    }];
    if skinned {
        entries.push(wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(sampler),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(skin),
        });
    }
    if let Some((cast_map, cast_sampler)) = cast {
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(cast_map),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::Sampler(cast_sampler),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("carbine object"),
        layout,
        entries: &entries,
    })
}

/// A pipeline for one surface format, with enough uniform slots for the frame.
fn ensure_pipeline<'a>(
    pipelines: &'a mut HashMap<wgpu::TextureFormat, Pipeline>,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    spec: &Spec,
    draw_count: usize,
) -> &'a mut Pipeline {
    let pipeline = pipelines.entry(format).or_insert_with(|| {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(spec.label),
            source: wgpu::ShaderSource::Wgsl(spec.source.into()),
        });
        // A cast writes depth and nothing else, so it is built with no fragment
        // stage at all rather than one that writes to no one.
        let color_target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let fragment = (!spec.depth_only).then(|| wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &color_target,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(spec.label),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("versus"),
                compilation_options: Default::default(),
                buffers: if spec.fullscreen {
                    &[]
                } else if spec.instanced {
                    &PARTICLE_LAYOUTS
                } else {
                    &VERTEX_LAYOUTS
                },
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Flat shapes such as a floor tile are authored to be seen from
                // either side, so nothing is culled.
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: spec.depth.then(|| wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(spec.depth_write),
                // Only a solid object claims the depth it lands in; translucent
                // objects and the backdrop are tested but never written.
                depth_compare: Some(if spec.fullscreen {
                    wgpu::CompareFunction::Always
                } else {
                    wgpu::CompareFunction::Less
                }),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment,
            multiview_mask: None,
            cache: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        Pipeline {
            pipeline,
            layout,
            uniform_bytes: spec.uniform_bytes as u64,
            uniforms: Vec::new(),
            skinned: spec.skinned,
        }
    });

    while pipeline.uniforms.len() < draw_count {
        pipeline
            .uniforms
            .push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("carbine uniforms"),
                size: pipeline.uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
    }

    pipeline
}

fn ensure_mesh<'a>(
    meshes: &'a mut HashMap<MeshHandle, GpuMesh>,
    device: &wgpu::Device,
    handle: MeshHandle,
    mesh: &MeshData,
) -> &'a GpuMesh {
    meshes.entry(handle).or_insert_with(|| {
        let vertices: Vec<Vertex> = mesh
            .positions
            .iter()
            .enumerate()
            .map(|(index, position)| Vertex {
                position: position.to_array(),
                normal: mesh.normals[index].to_array(),
                uv: mesh.uvs[index].to_array(),
            })
            .collect();
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("carbine vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("carbine indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        GpuMesh {
            vertex,
            index,
            index_count: mesh.indices.len() as u32,
        }
    })
}

/// A packed image goes onto the GPU as the bytes it already is: BC7 blocks, every
/// level of its mip chain, with no decoder between the file and video memory.
///
/// The chain's own transfer flag stays in the file. Carbine's pipeline is
/// display-referred — a colour reaches the screen exactly as it is stored — so a
/// texel has to do the same, and an sRGB-decoded sample would darken every picture.
fn upload_packed(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    texture: &carbine_ktx::Texture<'_>,
) -> wgpu::TextureView {
    let gpu = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: texture.width(),
            height: texture.height(),
            depth_or_array_layers: 1,
        },
        mip_level_count: texture.levels().len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bc7RgbaUnorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, blocks) in texture.levels().iter().enumerate() {
        // BC7 pads an edge that is not a whole block out to one, and the queue counts a
        // copy in whole blocks, so the size handed over is the padded one. The texels
        // past the picture live in blocks the sampler is never pointed at.
        let (rows, columns) = (blocks.height.div_ceil(4), blocks.width.div_ceil(4));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            blocks.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(columns * 16),
                rows_per_image: Some(rows),
            },
            wgpu::Extent3d {
                width: columns * 4,
                height: rows * 4,
                depth_or_array_layers: 1,
            },
        );
    }
    gpu.create_view(&wgpu::TextureViewDescriptor::default())
}

/// One uncompressed RGBA8 picture, which is what the stand-in white pixel is.
fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    bytes: &[u8],
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width.max(1) * 4),
            rows_per_image: Some(height.max(1)),
        },
        wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];

static VERTEX_LAYOUTS: [Option<wgpu::VertexBufferLayout<'static>>; 1] =
    [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    })];

/// The four corners of a quad, in its own axes, and the two triangles cut from
/// them. Every drop in a crowd is this same square, stood up to face the eyes.
const QUAD_CORNERS: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
const QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];

const QUAD_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];

static PARTICLE_LAYOUTS: [Option<wgpu::VertexBufferLayout<'static>>; 2] = [
    Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<[f32; 2]>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &QUAD_ATTRIBUTES,
    }),
    Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ParticleInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &INSTANCE_ATTRIBUTES,
    }),
];

/// The quad every instanced drop is cut from, made once per device.
fn unit_quad(device: &wgpu::Device) -> GpuMesh {
    let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("carbine crowd quad"),
        contents: bytemuck::cast_slice(&QUAD_CORNERS),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("carbine crowd indices"),
        contents: bytemuck::cast_slice(&QUAD_INDICES),
        usage: wgpu::BufferUsages::INDEX,
    });
    GpuMesh {
        vertex,
        index,
        index_count: QUAD_INDICES.len() as u32,
    }
}

/// Uploads a frame's crowd, growing the instance buffer only when more things are
/// in the air than the last frame had room for.
fn ensure_crowd(
    crowd: &mut Option<wgpu::Buffer>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bytes: &[u8],
) -> wgpu::Buffer {
    let needed = bytes.len() as u64;
    let room = crowd.as_ref().map_or(0, |buffer| buffer.size());
    if room < needed {
        *crowd = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("carbine crowd"),
            size: needed,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    let buffer = crowd
        .as_ref()
        .expect("a crowd that was asked to be uploaded has no buffer");
    queue.write_buffer(buffer, 0, bytes);
    buffer.clone()
}
