//! GLB — the only model format Carbine reads.
//!
//! A `.glb` is a 12 byte header, one JSON chunk that describes a scene and one binary chunk
//! holding every vertex. It is the whole glTF format squeezed into a single file, which is why
//! it is the one Carbine parses: no sidecars, no image codecs, no third-party reader standing
//! between a downloaded asset and a draw call. Anything authored in another format comes through
//! the Packager and lands here.
//!
//! The hierarchy survives the read. `Model` is a list of nodes with children, exactly as the file
//! spells it, and the core turns each one into a real scene node — an imported jet stays a jet
//! with a turret under it, not one welded mesh with the turret's triangles in it.
//!
//! glTF measures Y-up; Carbine measures Z-up. Every position, normal and node transform is
//! carried through the same rotation, so a parent that aimed its child somewhere in glTF still
//! aims it there after the flip.

mod error;
mod gltf;
mod json;
mod writer;

pub use error::{Error, Result};
pub use writer::write;

use glam::{Quat, Vec2, Vec3};
use json::Json;

/// `glTF` — the GLB magic, at the front of the file and nowhere else.
pub const MAGIC: [u8; 4] = *b"glTF";
/// The chunk type for the JSON descriptor: `"JSON"` read as a little-endian u32.
pub const JSON_CHUNK: u32 = 0x4E4F_534A;
/// The chunk type for the binary buffer: `"BIN\0"` read as a little-endian u32.
pub const BIN_CHUNK: u32 = 0x004E_4942;
/// Bytes before the first chunk.
const HEADER: usize = 12;
/// Bytes in front of each chunk body.
const CHUNK_HEADER: usize = 8;

/// glTF's six accessor component types, by the numbers the format uses.
pub(crate) const BYTE: u32 = 5120;
pub(crate) const UBYTE: u32 = 5121;
pub(crate) const SHORT: u32 = 5122;
pub(crate) const USHORT: u32 = 5123;
pub(crate) const UINT: u32 = 5125;
pub(crate) const FLOAT: u32 = 5126;

/// The rotation from glTF's Y-up to Carbine's Z-up: `(x, y, z) -> (x, -z, y)`.
///
/// Written as a matrix rather than as a quarter-turn quaternion because every entry is 0, 1 or
/// -1. A `cos(90°)` in `f32` is not quite zero, and a mesh that only ever crosses this way would
/// come out a tenth of a micrometre off its own corners for no reason.
pub(crate) fn up_fix() -> glam::Mat4 {
    glam::Mat4::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ])
}

/// One node's place relative to its parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn matrix(&self) -> glam::Mat4 {
        glam::Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    pub fn from_matrix(matrix: glam::Mat4) -> Transform {
        let (scale, rotation, position) = matrix.to_scale_rotation_translation();
        Transform {
            position,
            rotation,
            scale,
        }
    }

    /// Carry a transform written against a Y-up world into the Z-up one Carbine draws.
    ///
    /// The node matrix is conjugated rather than multiplied, which is what keeps composition
    /// honest: a flipped parent followed by a flipped child still lands the child where the glTF
    /// scene put it, so a turret keeps pointing at the hatch it hung over.
    fn to_z_up(self) -> Transform {
        let flip = up_fix();
        Transform::from_matrix(flip * self.matrix() * flip.transpose())
    }

    /// The same crossing travelled backwards, for writing a model out again.
    fn to_y_up(self) -> Transform {
        let flip = up_fix();
        Transform::from_matrix(flip.transpose() * self.matrix() * flip)
    }
}

/// Surface colour. glTF's `baseColorFactor` is linear light and Carbine's palette is
/// display-referred, so the components arrive already moved into the space the renderer expects.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: Option<String>,
    /// Red, green, blue and alpha, each 0.0 to 1.0.
    pub color: [f32; 4],
}

impl Material {
    /// White and solid: what a primitive with no material of its own is drawn as.
    pub const DEFAULT: Material = Material {
        name: None,
        color: [1.0, 1.0, 1.0, 1.0],
    };
}

/// One drawable piece of one node: the triangles it is made of and how they are shaded.
#[derive(Clone, Debug)]
pub struct Part {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Zeroes wherever the file carries no texture coordinates.
    pub uvs: Vec<Vec2>,
    /// Indexes into `positions`, widened to `u32` however narrow the file stored them.
    pub indices: Vec<u32>,
    pub material: Material,
}

impl Part {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The box the part fills in its own node's space, or `None` when it holds no vertices.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for position in &self.positions {
            min = min.min(*position);
            max = max.max(*position);
        }
        if self.positions.is_empty() {
            None
        } else {
            Some((min, max))
        }
    }
}

/// A named place in the model, holding parts and pointing at children.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: Option<String>,
    pub transform: Transform,
    pub children: Vec<usize>,
    pub parts: Vec<Part>,
}

impl Default for Node {
    fn default() -> Node {
        Node {
            name: None,
            transform: Transform::IDENTITY,
            children: Vec::new(),
            parts: Vec::new(),
        }
    }
}

/// A model read out of a GLB: nodes, and the top-level ones among them.
#[derive(Clone, Debug)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
}

impl Model {
    pub fn new(nodes: Vec<Node>, roots: Vec<usize>) -> Model {
        Model { nodes, roots }
    }

    pub fn part_count(&self) -> usize {
        self.nodes.iter().map(|node| node.parts.len()).sum()
    }

    pub fn vertex_count(&self) -> usize {
        self.nodes
            .iter()
            .flat_map(|node| node.parts.iter())
            .map(|part| part.positions.len())
            .sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.nodes
            .iter()
            .flat_map(|node| node.parts.iter())
            .map(Part::triangle_count)
            .sum()
    }

    /// The first node carrying this name, whichever branch it sits in.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.name.as_deref() == Some(name))
    }

    /// The model as one root and everything reachable from it, renumbered from zero.
    ///
    /// Dropping what is unreachable is the point: a file that ships twelve vehicles in one scene
    /// hands over only the one that was asked for, standing where it stood before.
    fn subtree(&self, root: usize) -> Model {
        let mut nodes: Vec<Node> = Vec::new();
        let mut slots = vec![usize::MAX; self.nodes.len()];
        let mut queue = vec![root];
        slots[root] = 0;
        while let Some(source) = queue.pop() {
            let target = slots[source];
            let node = &self.nodes[source];
            let mut children = Vec::with_capacity(node.children.len());
            // Hold this node's slot before handing out children, or the first child lands on it.
            if nodes.len() <= target {
                nodes.resize(target + 1, Node::default());
            }
            for child in &node.children {
                let child = *child;
                let slot = slots[child];
                if slot == usize::MAX {
                    slots[child] = nodes.len();
                    nodes.push(Node::default());
                    queue.push(child);
                }
                children.push(slots[child]);
            }
            nodes[target] = Node {
                name: node.name.clone(),
                transform: node.transform,
                children,
                parts: node.parts.clone(),
            };
        }
        Model::new(nodes, vec![0])
    }
}

/// True once a buffer is long enough to start with the GLB magic.
pub fn is_glb(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER && bytes.starts_with(&MAGIC)
}

/// Read a whole GLB in memory as a Z-up model tree.
pub fn read(bytes: &[u8]) -> Result<Model> {
    read_scene(bytes, None)
}

/// Read a GLB from disk.
pub fn read_file(path: impl AsRef<std::path::Path>) -> Result<Model> {
    read(&std::fs::read(path)?)
}

/// Read a GLB, optionally narrowing it to one named node and the branch under it.
///
/// A file with a single scene root comes back unchanged. One holding a hangar full of vehicles
/// can be asked for `"Heavy Tank"` and arrives as that tank alone.
pub fn read_scene(bytes: &[u8], root: Option<&str>) -> Result<Model> {
    let (json_bytes, bin) = chunks(bytes)?;
    let text =
        std::str::from_utf8(json_bytes).map_err(|_| Error::Json("it is not UTF-8".into()))?;
    let value = Json::parse(text).map_err(Error::Json)?;
    let model = gltf::build(&value, bin)?;
    match root {
        None => Ok(model),
        Some(name) => model
            .find(name)
            .map(|index| model.subtree(index))
            .ok_or_else(|| Error::NodeNotFound(name.into())),
    }
}

/// Split a GLB into its JSON and binary chunks, checking the container on the way.
fn chunks(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    if bytes.len() < HEADER {
        return Err(Error::TooShort);
    }
    if !bytes.starts_with(&MAGIC) {
        return Err(Error::NotGlb);
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != 2 {
        return Err(Error::Version(version));
    }
    let declared = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if declared > bytes.len() {
        return Err(Error::Truncated("file"));
    }
    // Work inside the length the header claims; anything past it is not part of the document.
    let file = &bytes[..declared];

    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    let mut at = HEADER;
    while at + CHUNK_HEADER <= file.len() {
        let length = u32::from_le_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap());
        let start = at + CHUNK_HEADER;
        if file.len() - start < length {
            return Err(Error::Truncated("chunk"));
        }
        // Chunks are padded out to a multiple of four; the padding is not content.
        let end = start + length;
        let body = &file[start..end];
        // Step over the padding whether the header counted it or not. Writers that declare the
        // unpadded length are common enough, and the padding belongs to the chunk either way.
        at = end.div_ceil(4) * 4;
        match kind {
            JSON_CHUNK if json.is_none() => json = Some(body),
            BIN_CHUNK if bin.is_none() => bin = Some(body),
            _ => {}
        }
    }

    let json = json.ok_or(Error::MissingJson)?;
    Ok((json, bin.unwrap_or(&[])))
}
