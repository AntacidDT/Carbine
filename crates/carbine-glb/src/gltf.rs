//! The walk from a glTF JSON header to a `Model`.
//!
//! Everything here translates the names the format uses into the shapes in `lib`. Values stay in
//! glTF's own space until the last step, when positions, normals and node transforms all go
//! through the same flip.

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::json::Json;
use crate::{
    BYTE, Error, FLOAT, Material, Model, Node, Part, Result, SHORT, Transform, UBYTE, UINT, USHORT,
    up_fix,
};

/// The tables a glTF header indexes into, already looked up.
struct File<'a> {
    accessors: Vec<&'a Json>,
    views: Vec<&'a Json>,
    materials: Vec<&'a Json>,
    meshes: Vec<&'a Json>,
    nodes: Vec<&'a Json>,
    bin: &'a [u8],
}

/// An entry of one of those tables, or a dangling-reference error naming it.
fn entry<'a>(table: &'static str, items: &'a [&'a Json], index: Option<usize>) -> Result<&'a Json> {
    let index = index.ok_or(Error::Dangling(table))?;
    items.get(index).copied().ok_or(Error::Dangling(table))
}

/// Build a model from the JSON chunk of a GLB and its binary buffer.
pub fn build(root: &Json, bin: &[u8]) -> Result<Model> {
    if !matches!(root, Json::Obj(_)) {
        return Err(Error::Json("the document is not an object".into()));
    }
    match root.get("asset").and_then(|a| a.get("version")) {
        Some(Json::Str(version)) if version.starts_with('2') => {}
        Some(Json::Str(version)) => {
            return Err(Error::Json(format!("glTF version {version} is not 2.x")));
        }
        _ => return Err(Error::Json("there is no asset.version".into())),
    }
    for required in array(root, "extensionsRequired") {
        if let Some(name) = required.as_str() {
            return Err(Error::Requires(name.into()));
        }
    }
    if !array(root, "skins").is_empty() {
        return Err(Error::Requires("a skeletal skin".into()));
    }
    if !array(root, "images").is_empty() {
        return Err(Error::Requires(
            "an embedded image. Pack its skins as .ktx2 and name them after the node".into(),
        ));
    }
    if array(root, "buffers")
        .iter()
        .any(|buffer| buffer.get("uri").is_some())
    {
        return Err(Error::Requires("a buffer stored outside the file".into()));
    }

    let file = File {
        accessors: table(root, "accessors"),
        views: table(root, "bufferViews"),
        materials: table(root, "materials"),
        meshes: table(root, "meshes"),
        nodes: table(root, "nodes"),
        bin,
    };

    let mut nodes = Vec::with_capacity(file.nodes.len());
    for value in &file.nodes {
        nodes.push(node(&file, value)?);
    }
    let roots = scene_roots(root, &file, &nodes)?;
    Ok(Model { nodes, roots })
}

/// The top-level nodes of the default scene, or every node nothing else claims as its parent.
fn scene_roots(root: &Json, file: &File, nodes: &[Node]) -> Result<Vec<usize>> {
    let scenes = table(root, "scenes");
    let chosen = match root.get("scene") {
        None => scenes.first().copied(),
        Some(index) => Some(entry("scenes", &scenes, index.as_index())?),
    };
    if let Some(scene) = chosen {
        let mut roots = Vec::new();
        for value in array(scene, "nodes") {
            let index = value
                .as_index()
                .ok_or(Error::Json("a scene node is not an index".into()))?;
            entry("nodes", &file.nodes, Some(index))?;
            roots.push(index);
        }
        if !roots.is_empty() {
            return Ok(roots);
        }
    }
    // A scene with nothing in it, or a file with no scene at all: infer roots from parent links.
    let mut claimed = vec![false; nodes.len()];
    for node in nodes {
        for child in &node.children {
            if let Some(slot) = claimed.get_mut(*child) {
                *slot = true;
            }
        }
    }
    let inferred: Vec<usize> = (0..nodes.len()).filter(|index| !claimed[*index]).collect();
    if inferred.is_empty() && !nodes.is_empty() {
        return Err(Error::Json(
            "every node has a parent and no scene names a root".into(),
        ));
    }
    Ok(inferred)
}

/// One node: its name, its place, its children, and the parts of whatever mesh it wears.
fn node(file: &File, value: &Json) -> Result<Node> {
    let mut node = Node {
        name: value.name(),
        transform: transform(value)?.to_z_up(),
        children: Vec::new(),
        parts: Vec::new(),
    };
    for child in array(value, "children") {
        let index = child
            .as_index()
            .ok_or(Error::Json("a child is not an index".into()))?;
        entry("nodes", &file.nodes, Some(index))?;
        node.children.push(index);
    }
    let mesh_index = value.get("mesh").and_then(Json::as_index);
    if let Some(mesh_index) = mesh_index {
        // A node pointing at a mesh the file does not hold is a broken file, not a decoration.
        let mesh = entry("meshes", &file.meshes, Some(mesh_index))?;
        for primitive in array(mesh, "primitives") {
            node.parts.push(part(file, primitive)?);
        }
    }
    Ok(node)
}

/// One primitive: its triangles, its surface, and the normals a file may not have written.
fn part(file: &File, primitive: &Json) -> Result<Part> {
    let mode = primitive.get("mode").and_then(Json::as_index).unwrap_or(4);
    if mode != 4 {
        return Err(Error::Mode(mode as u32));
    }
    if primitive.get("targets").is_some() {
        return Err(Error::Requires("a morph target".into()));
    }
    let attributes = primitive
        .get("attributes")
        .ok_or(Error::Json("a primitive has no attributes".into()))?;
    let position = attributes
        .get("POSITION")
        .and_then(Json::as_index)
        .ok_or(Error::MissingPosition)?;
    let accessor = entry("accessors", &file.accessors, Some(position))?;
    let positions = file.vec3(accessor)?;

    let indices = match primitive.get("indices").and_then(Json::as_index) {
        Some(index) => {
            let accessor = entry("accessors", &file.accessors, Some(index))?;
            file.indices(accessor, positions.len())?
        }
        // A primitive with no index list is already a flat run of triangles.
        None => (0..positions.len() as u32).collect(),
    };

    let normals = match attributes.get("NORMAL").and_then(Json::as_index) {
        Some(index) => file.vec3(entry("accessors", &file.accessors, Some(index))?)?,
        None => face_normals(&positions, &indices),
    };

    let uvs = match attributes.get("TEXCOORD_0").and_then(Json::as_index) {
        Some(index) => file.vec2(entry("accessors", &file.accessors, Some(index))?)?,
        None => vec![Vec2::ZERO; positions.len()],
    };

    let material = match primitive.get("material").and_then(Json::as_index) {
        Some(index) => material(entry("materials", &file.materials, Some(index))?),
        // A primitive that names no material at all is plain white.
        None => Material::DEFAULT,
    };

    let flip = up_fix();
    Ok(Part {
        positions: positions
            .iter()
            .map(|position| flip.transform_point3(*position))
            .collect(),
        normals: normals
            .iter()
            .map(|normal| flip.transform_vector3(*normal))
            .collect(),
        uvs,
        indices,
        material,
    })
}

/// A colour in the space the renderer works in.
///
/// glTF writes `baseColorFactor` as linear light and Carbine's whole palette is display-referred,
/// so the three colour channels are gamma-encoded on the way in. Alpha stays as the file wrote it
/// — it is a coverage rather than a colour, and the shader blend treats it as one. A `MASK`
/// material keeps its colour and is drawn solid, because there is no discard anywhere here.
fn material(value: &Json) -> Material {
    let factor = value
        .get("pbrMetallicRoughness")
        .and_then(|p| p.get("baseColorFactor"))
        .and_then(Json::as_array)
        .unwrap_or(&[]);
    let channel = |index: usize, fallback: f32| {
        factor
            .get(index)
            .and_then(Json::as_f32)
            .unwrap_or(fallback)
            .clamp(0.0, 1.0)
    };
    let blended = value.get("alphaMode").and_then(Json::as_str) == Some("BLEND");
    Material {
        name: value.name(),
        color: [
            encode_srgb(channel(0, 1.0)),
            encode_srgb(channel(1, 1.0)),
            encode_srgb(channel(2, 1.0)),
            if blended { channel(3, 1.0) } else { 1.0 },
        ],
    }
}

fn encode_srgb(linear: f32) -> f32 {
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// Normals built from the triangles themselves, for a file that shipped none.
///
/// Each face normal is added to its three corners weighted by its own area, which is the cross
/// product's length, and the sums are normalised. A corner no triangle reaches gets the up axis
/// instead of a NaN.
fn face_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let corner = |slot: usize| {
            triangle
                .get(slot)
                .map(|index| *index as usize)
                .and_then(|index| positions.get(index))
        };
        let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
            continue;
        };
        let face = (*b - *a).cross(*c - *a);
        for index in triangle {
            if let Some(slot) = normals.get_mut(*index as usize) {
                *slot += face;
            }
        }
    }
    normals
        .into_iter()
        .map(|normal| {
            if normal.length_squared() > 1e-12 {
                normal.normalize()
            } else {
                Vec3::Z
            }
        })
        .collect()
}

/// The two ways a node can state its place, reduced to one.
fn transform(value: &Json) -> Result<Transform> {
    if let Some(columns) = floats::<16>(value.get("matrix")) {
        // glTF stores a matrix column-major, which is exactly how glam lays one out.
        return Ok(Transform::from_matrix(Mat4::from_cols_array(&columns)));
    }
    let position = floats::<3>(value.get("translation"))
        .map(Vec3::from)
        .unwrap_or(Vec3::ZERO);
    let scale = floats::<3>(value.get("scale"))
        .map(Vec3::from)
        .unwrap_or(Vec3::ONE);
    let rotation = match floats::<4>(value.get("rotation")) {
        Some([x, y, z, w]) => Quat::from_xyzw(x, y, z, w).normalize(),
        None => Quat::IDENTITY,
    };
    Ok(Transform {
        position,
        rotation,
        scale,
    })
}

/// A fixed-length run of numbers, or `None` if the value is absent or the wrong length.
fn floats<const N: usize>(value: Option<&Json>) -> Option<[f32; N]> {
    let numbers = value.and_then(Json::as_array)?;
    if numbers.len() != N {
        return None;
    }
    let mut out = [0.0f32; N];
    for (slot, item) in out.iter_mut().zip(numbers) {
        *slot = item.as_f32()?;
    }
    Some(out)
}

/// The array under a key, empty when the key is absent or is not an array.
fn array<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    value.get(key).and_then(Json::as_array).unwrap_or(&[])
}

/// The entries of a table, owned so the walk can hand out references without a lifetime fight.
fn table<'a>(root: &'a Json, key: &str) -> Vec<&'a Json> {
    array(root, key).iter().collect()
}

impl File<'_> {
    /// Where one accessor's elements live: which bytes, how far apart, how many, how wide.
    fn plan<'a>(&'a self, accessor: &'a Json, types: &[&str], kinds: &[u32]) -> Result<Plan<'a>> {
        let sparse_count = accessor
            .get("sparse")
            .and_then(|sparse| sparse.get("count"))
            .and_then(Json::as_index)
            .unwrap_or(0);
        if sparse_count > 0 {
            return Err(Error::Sparse);
        }
        let kind = accessor
            .get("componentType")
            .and_then(Json::as_index)
            .ok_or(Error::Json("an accessor has no componentType".into()))?
            as u32;
        if !kinds.contains(&kind) {
            return Err(Error::ComponentType(kind));
        }
        let shape = accessor.get("type").and_then(Json::as_str).unwrap_or("");
        if !types.contains(&shape) {
            return Err(Error::AccessorType(shape.to_string()));
        }
        let width = match shape {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => return Err(Error::AccessorType(shape.to_string())),
        };
        let count = accessor
            .get("count")
            .and_then(Json::as_index)
            .ok_or(Error::Json("an accessor has no count".into()))?;
        let view_index = accessor
            .get("bufferView")
            .and_then(Json::as_index)
            .ok_or(Error::Requires("an accessor with no buffer view".into()))?;
        let view = entry("bufferViews", &self.views, Some(view_index))?;
        let element = element(kind);
        let offset = accessor
            .get("byteOffset")
            .and_then(Json::as_index)
            .unwrap_or(0);
        let start = view.get("byteOffset").and_then(Json::as_index).unwrap_or(0) + offset;
        // A planar accessor leaves the stride out; a missing or silly stride means packed.
        let stride = view.get("byteStride").and_then(Json::as_index).unwrap_or(0);
        let stride = if stride >= width * element {
            stride
        } else {
            width * element
        };
        let span = if count == 0 {
            0
        } else {
            stride * (count - 1) + width * element
        };
        if start
            .checked_add(span)
            .is_none_or(|end| end > self.bin.len())
        {
            return Err(Error::Truncated("accessor"));
        }
        Ok(Plan {
            base: &self.bin[start..],
            stride,
            count,
            kind,
        })
    }

    fn vec3(&self, accessor: &Json) -> Result<Vec<Vec3>> {
        let plan = self.plan(accessor, &["VEC3"], &[FLOAT])?;
        Ok((0..plan.count)
            .map(|index| {
                let at = index * plan.stride;
                Vec3::new(plan.float(at), plan.float(at + 4), plan.float(at + 8))
            })
            .collect())
    }

    fn vec2(&self, accessor: &Json) -> Result<Vec<Vec2>> {
        let plan = self.plan(accessor, &["VEC2"], &[FLOAT])?;
        Ok((0..plan.count)
            .map(|index| {
                let at = index * plan.stride;
                Vec2::new(plan.float(at), plan.float(at + 4))
            })
            .collect())
    }

    /// Triangle indexes, widened to `u32` from whatever narrow type the file chose.
    fn indices(&self, accessor: &Json, vertices: usize) -> Result<Vec<u32>> {
        let plan = self.plan(accessor, &["SCALAR"], &[UBYTE, USHORT, UINT])?;
        let mut out = Vec::with_capacity(plan.count);
        for index in 0..plan.count {
            let value = plan.integer(index * plan.stride);
            if value as usize >= vertices {
                return Err(Error::IndexOutOfRange {
                    index: value,
                    vertices,
                });
            }
            out.push(value);
        }
        Ok(out)
    }
}

/// One accessor's decoded element run, as a window on the binary chunk.
struct Plan<'a> {
    base: &'a [u8],
    stride: usize,
    count: usize,
    kind: u32,
}

impl Plan<'_> {
    /// A component the accessor range was already checked against, so a short read means zero.
    fn float(&self, at: usize) -> f32 {
        self.bytes(at, 4)
            .map(|raw| f32::from_le_bytes(raw.try_into().unwrap()))
            .unwrap_or(0.0)
    }

    fn integer(&self, at: usize) -> u32 {
        let width = element(self.kind);
        let Some(raw) = self.bytes(at, width) else {
            return 0;
        };
        let signed = matches!(self.kind, BYTE | SHORT);
        match width {
            1 => {
                let value = raw[0];
                if signed {
                    value as i8 as u32
                } else {
                    value as u32
                }
            }
            2 => {
                let value = u16::from_le_bytes(raw.try_into().unwrap());
                if signed {
                    value as i16 as u32
                } else {
                    value as u32
                }
            }
            4 => u32::from_le_bytes(raw.try_into().unwrap()),
            _ => 0,
        }
    }

    fn bytes(&self, at: usize, width: usize) -> Option<&[u8]> {
        self.base.get(at..at.checked_add(width)?)
    }
}

fn element(kind: u32) -> usize {
    match kind {
        BYTE | UBYTE => 1,
        SHORT | USHORT => 2,
        _ => 4,
    }
}
