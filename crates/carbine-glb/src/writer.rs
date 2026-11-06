//! Writing a `Model` back out as a GLB.
//!
//! Two things need this: the fixtures that prove the reader works, and the Packager's `model()`
//! call, which converts an authored `.obj` or `.stl` into the one format the engine reads. The
//! writer is the reader's mirror, so a model that goes through `write` and comes back through
//! `read` is the same model — that round-trip is the test.
//!
//! Only what `Model` actually holds is written: nodes, transforms, triangles and a flat colour.
//! Skins, animations, textures and morph targets have nowhere to live in this crate, so a file
//! carrying them comes back out without them. That is the cleaning the Packager is for.

use glam::{Quat, Vec2, Vec3};

use crate::json::Json;
use crate::{BIN_CHUNK, FLOAT, JSON_CHUNK, MAGIC, Material, Model, Part, UINT, up_fix};

/// Turn a model into the bytes of a GLB file, Z-up carried back into glTF's Y-up on the way.
pub fn write(model: &Model) -> Vec<u8> {
    let mut bin = Bin::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();

    for node in &model.nodes {
        let mut primitives = Vec::new();
        for part in &node.parts {
            if !part.positions.is_empty() {
                primitives.push(primitive(&mut bin, part));
            }
        }
        let transform = node.transform.to_y_up();
        let mut members = Vec::new();
        if let Some(name) = &node.name {
            members.push(field("name", Json::Str(name.clone())));
        }
        if transform.position != Vec3::ZERO {
            members.push(field("translation", vector(transform.position.to_array())));
        }
        if transform.rotation != Quat::IDENTITY {
            members.push(field("rotation", vector(transform.rotation.to_array())));
        }
        if transform.scale != Vec3::ONE {
            members.push(field("scale", vector(transform.scale.to_array())));
        }
        if !primitives.is_empty() {
            let index = meshes.len();
            meshes.push(Json::Obj(vec![field("primitives", Json::Arr(primitives))]));
            members.push(field("mesh", number(index)));
        }
        if !node.children.is_empty() {
            members.push(field(
                "children",
                Json::Arr(node.children.iter().map(|child| number(*child)).collect()),
            ));
        }
        nodes.push(Json::Obj(members));
    }

    let roots = model
        .roots
        .iter()
        .map(|index| number(*index))
        .collect::<Vec<Json>>();
    let header = Json::Obj(vec![
        field(
            "asset",
            Json::Obj(vec![field("version", Json::Str("2.0".into()))]),
        ),
        field("scene", number(0)),
        field(
            "scenes",
            Json::Arr(vec![Json::Obj(vec![field("nodes", Json::Arr(roots))])]),
        ),
        field("nodes", Json::Arr(nodes)),
        field("meshes", Json::Arr(meshes)),
        field(
            "materials",
            Json::Arr(bin.materials.into_iter().map(Json::Obj).collect()),
        ),
        field("accessors", Json::Arr(bin.accessors)),
        field("bufferViews", Json::Arr(bin.views)),
        field(
            "buffers",
            Json::Arr(vec![Json::Obj(vec![field(
                "byteLength",
                number(bin.bytes.len()),
            )])]),
        ),
    ]);

    let json = pad(header.dump().into_bytes(), b' ');
    let data = pad(bin.bytes, 0);
    let total = 12 + 8 + json.len() + 8 + data.len();

    let mut out: Vec<u8> = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&JSON_CHUNK.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&BIN_CHUNK.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// One primitive: three attributes, an index run and a flat colour.
fn primitive(bin: &mut Bin, part: &Part) -> Json {
    let flip = up_fix().transpose();
    let positions: Vec<Vec3> = part
        .positions
        .iter()
        .map(|position| flip.transform_point3(*position))
        .collect();
    let normals: Vec<Vec3> = part
        .normals
        .iter()
        .map(|normal| flip.transform_vector3(*normal))
        .collect();

    let position = bin.push_vec3(&positions);
    let normal = bin.push_vec3(&normals);
    let uv = bin.push_vec2(&part.uvs);
    let indices = bin.push_indices(&part.indices);
    let material = bin.push_material(&part.material);

    Json::Obj(vec![
        field(
            "attributes",
            Json::Obj(vec![
                field("POSITION", number(position)),
                field("NORMAL", number(normal)),
                field("TEXCOORD_0", number(uv)),
            ]),
        ),
        field("indices", number(indices)),
        field("material", number(material)),
        field("mode", number(4)),
    ])
}

fn vector<const N: usize>(values: [f32; N]) -> Json {
    Json::Arr(
        values
            .iter()
            .map(|value| Json::Num(*value as f64))
            .collect(),
    )
}

fn number(value: usize) -> Json {
    Json::Num(value as f64)
}

fn field(key: &str, value: Json) -> (String, Json) {
    (key.to_string(), value)
}

/// Pad a chunk out to the multiple of four the container requires.
fn pad(mut bytes: Vec<u8>, fill: u8) -> Vec<u8> {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(fill);
    }
    bytes
}

/// The binary chunk and the tables that index into it, grown one array at a time.
struct Bin {
    bytes: Vec<u8>,
    views: Vec<Json>,
    accessors: Vec<Json>,
    materials: Vec<Vec<(String, Json)>>,
}

impl Bin {
    fn new() -> Bin {
        Bin {
            bytes: Vec::new(),
            views: Vec::new(),
            accessors: Vec::new(),
            materials: Vec::new(),
        }
    }

    fn push_vec3(&mut self, values: &[Vec3]) -> usize {
        let mut payload = Vec::with_capacity(values.len() * 12);
        for value in values {
            for channel in value.to_array() {
                payload.extend_from_slice(&channel.to_le_bytes());
            }
        }
        let (min, max) = bounds(values);
        self.push(payload, values.len(), "VEC3", FLOAT, min, max)
    }

    fn push_vec2(&mut self, values: &[Vec2]) -> usize {
        let mut payload = Vec::with_capacity(values.len() * 8);
        for value in values {
            for channel in value.to_array() {
                payload.extend_from_slice(&channel.to_le_bytes());
            }
        }
        self.push(payload, values.len(), "VEC2", FLOAT, None, None)
    }

    fn push_indices(&mut self, values: &[u32]) -> usize {
        let mut payload = Vec::with_capacity(values.len() * 4);
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        self.push(payload, values.len(), "SCALAR", UINT, None, None)
    }

    /// Append one array's bytes and register the view and accessor that describe them.
    fn push(
        &mut self,
        payload: Vec<u8>,
        count: usize,
        shape: &str,
        kind: u32,
        min: Option<[f32; 3]>,
        max: Option<[f32; 3]>,
    ) -> usize {
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(&payload);
        self.views.push(Json::Obj(vec![
            field("buffer", number(0)),
            field("byteOffset", number(offset)),
            field("byteLength", number(payload.len())),
        ]));
        let view = self.views.len() - 1;
        let mut members = vec![
            field("bufferView", number(view)),
            field("byteOffset", number(0)),
            field("componentType", number(kind as usize)),
            field("count", number(count)),
            field("type", Json::Str(shape.to_string())),
        ];
        // A position accessor records its own box, which is the first thing a loader wants.
        if let (Some(min), Some(max)) = (min, max) {
            members.push(field("min", vector(min)));
            members.push(field("max", vector(max)));
        }
        self.accessors.push(Json::Obj(members));
        self.accessors.len() - 1
    }

    /// Colours go back out as linear light, which is where they came from.
    fn push_material(&mut self, material: &Material) -> usize {
        let [r, g, b, a] = material.color;
        let mut members = Vec::new();
        if let Some(name) = &material.name {
            members.push(field("name", Json::Str(name.clone())));
        }
        if a < 1.0 {
            members.push(field("alphaMode", Json::Str("BLEND".into())));
        }
        members.push(field(
            "pbrMetallicRoughness",
            Json::Obj(vec![field(
                "baseColorFactor",
                Json::Arr(vec![
                    Json::Num(decode_srgb(r)),
                    Json::Num(decode_srgb(g)),
                    Json::Num(decode_srgb(b)),
                    Json::Num(a as f64),
                ]),
            )]),
        ));
        self.materials.push(members);
        self.materials.len() - 1
    }
}

fn bounds(values: &[Vec3]) -> (Option<[f32; 3]>, Option<[f32; 3]>) {
    if values.is_empty() {
        return (None, None);
    }
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    (Some(min.to_array()), Some(max.to_array()))
}

/// The inverse of the reader's gamma step.
fn decode_srgb(display: f32) -> f64 {
    let value = display.clamp(0.0, 1.0) as f64;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
