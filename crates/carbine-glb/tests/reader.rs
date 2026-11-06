//! The GLB reader, tested against files assembled here rather than against this crate's own
//! writer — a reader and a writer that agree with each other prove nothing about glTF.

use std::f32::consts::FRAC_PI_2;

use carbine_glb::{Error, Material, Model, Node, Part, Transform, is_glb, read, read_scene, write};
use glam::{Quat, Vec2, Vec3};

/// Round a chunk length up to the multiple of four the container requires.
fn pad(len: usize) -> usize {
    len.div_ceil(4) * 4
}

/// A GLB with this JSON and this binary chunk, each padded out to a whole four bytes.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    glb_version(2, json, bin)
}

/// The same, with the version field forced to whatever a test wants refused.
///
/// The chunk lengths are the padded ones, which is what a conformant writer declares.
fn glb_version(version: u32, json: &str, bin: &[u8]) -> Vec<u8> {
    let head = 12 + 8 + pad(json.len());
    let total = head + 8 + pad(bin.len());
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(pad(json.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json.as_bytes());
    out.resize(head, b' ');
    out.extend_from_slice(&(pad(bin.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(bin);
    out.resize(total, 0);
    out
}

fn f32s(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn u16s(values: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 2);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// A triangle lying in glTF's floor plane, with its corners in order.
const SHEET: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// One node wearing one mesh of one primitive, with only positions and 16-bit indexes.
const SHEET_JSON: &str = r#"{
  "asset": {"version": "2.0"},
  "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36},
                  {"buffer": 0, "byteOffset": 36, "byteLength": 6}],
  "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}],
  "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
  "nodes": [{"name": "Sheet", "mesh": 0}],
  "scenes": [{"nodes": [0]}],
  "scene": 0
}"#;

/// The same triangle as a finished binary chunk.
fn sheet() -> Vec<u8> {
    let mut bin = f32s(&SHEET);
    bin.extend_from_slice(&u16s(&[0, 1, 2]));
    bin
}

#[test]
fn turns_the_triangle_up_and_widens_its_indexes() {
    let model = read(&glb(SHEET_JSON, &sheet())).unwrap();

    assert_eq!(model.roots, vec![0]);
    assert_eq!(model.nodes.len(), 1);
    assert_eq!(model.nodes[0].name.as_deref(), Some("Sheet"));
    assert_eq!(model.part_count(), 1);
    let part = &model.nodes[0].parts[0];
    // glTF's (x, y, z) becomes Carbine's (x, -z, y): the sheet stands up on the XZ floor.
    assert_eq!(part.positions, vec![Vec3::ZERO, Vec3::X, Vec3::Z]);
    assert_eq!(part.indices, vec![0, 1, 2]);
    assert_eq!(part.triangle_count(), 1);
    assert_eq!(model.triangle_count(), 1);
    assert_eq!(model.vertex_count(), 3);
    assert!(model.nodes[0].children.is_empty());
}

#[test]
fn makes_normals_and_texture_coordinates_a_file_never_wrote() {
    let part = &read(&glb(SHEET_JSON, &sheet())).unwrap().nodes[0].parts[0];

    // The face points at glTF's +Z, which is Carbine's -Y once the file is stood up.
    assert!(
        part.normals.iter().all(|normal| *normal == Vec3::NEG_Y),
        "every corner of a flat triangle faces the same way"
    );
    assert_eq!(part.uvs, vec![Vec2::ZERO; 3]);
    assert_eq!(part.material, Material::DEFAULT);
}

#[test]
fn reads_a_tiny_index_type_and_a_big_one() {
    let mut bin = f32s(&SHEET);
    bin.extend_from_slice(&[0, 1, 2]);
    let json = SHEET_JSON.replace("5123", "5121");
    let model = read(&glb(&json, &bin)).unwrap();
    assert_eq!(model.nodes[0].parts[0].indices, vec![0, 1, 2]);

    let mut bin = f32s(&SHEET);
    for index in [0u32, 1, 2] {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    let json = SHEET_JSON
        .replace("5123", "5125")
        .replace("\"byteLength\": 6", "\"byteLength\": 12");
    let model = read(&glb(&json, &bin)).unwrap();
    assert_eq!(model.nodes[0].parts[0].indices, vec![0, 1, 2]);
}

#[test]
fn reads_an_interleaved_accessor_by_its_stride() {
    // One view, three vertices, each twelve bytes of position then twelve of normal.
    let json = r#"{
  "asset": {"version": "2.0"},
  "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 72, "byteStride": 24}],
  "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": 3, "type": "VEC3"}],
  "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]}],
  "nodes": [{"mesh": 0}],
  "scenes": [{"nodes": [0]}]
}"#;
    let mut bin = Vec::new();
    for position in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bin.extend_from_slice(&f32s(&position));
        bin.extend_from_slice(&f32s(&[0.0, 1.0, 0.0]));
    }
    let part = &read(&glb(json, &bin)).unwrap().nodes[0].parts[0];

    assert_eq!(part.positions, vec![Vec3::ZERO, Vec3::X, Vec3::Z]);
    // A normal that pointed up the glTF file now points up the Carbine world.
    assert!(part.normals.iter().all(|normal| *normal == Vec3::Z));
    // With no index list, the vertices are already a flat run of triangles.
    assert_eq!(part.indices, vec![0, 1, 2]);
}

#[test]
fn reads_a_matrix_node() {
    let json = r#"{
  "asset": {"version": "2.0"},
  "nodes": [{"name": "Slide", "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 2, 0, 0, 1]}],
  "scenes": [{"nodes": [0]}]
}"#;
    let model = read(&glb(json, &[])).unwrap();
    assert_eq!(model.nodes[0].transform.position, Vec3::new(2.0, 0.0, 0.0));
    assert_eq!(model.nodes[0].transform.scale, Vec3::ONE);
}

#[test]
fn keeps_a_rotated_branch_pointing_the_same_way() {
    // In glTF: a hull five metres up the Y axis, turned a quarter turn about that axis, with a
    // turret three metres out along the hull's own X. That puts the turret at (0, 5, -3).
    let quarter = (FRAC_PI_2 / 2.0).sin();
    let json = format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "nodes": [
    {{"name": "Turret", "translation": [3.0, 0.0, 0.0]}},
    {{"name": "Hull", "children": [0], "translation": [0.0, 5.0, 0.0],
      "rotation": [0.0, {quarter}, 0.0, {quarter}]}}
  ],
  "scenes": [{{"nodes": [1]}}]
}}"#
    );
    let model = read(&glb(&json, &[])).unwrap();

    assert_eq!(model.nodes.len(), 2);
    assert_eq!(model.roots, vec![1]);
    assert_eq!(model.nodes[1].children, vec![0]);
    // Standing the whole branch up moves the hull to five metres of Z and swings the turret to
    // (0, 3, 5): the same point in space, told in the other axis order.
    let world = model.nodes[1].transform.matrix() * model.nodes[0].transform.matrix();
    let turret = world.transform_point3(Vec3::ZERO);
    assert!(
        (turret - Vec3::new(0.0, 3.0, 5.0)).length() < 1e-5,
        "the turret ended up at {turret:?}"
    );
}

#[test]
fn reads_a_colour_in_the_space_the_renderer_works_in() {
    let json = r#"{
  "asset": {"version": "2.0"},
  "materials": [
    {"name": "Mid", "pbrMetallicRoughness": {"baseColorFactor": [0.2, 0.2, 0.2, 1.0]}},
    {"name": "Glass", "alphaMode": "BLEND",
     "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 0.5]}},
    {"name": "Solid", "alphaMode": "OPAQUE",
     "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 0.5]}}
  ],
  "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0},
                             {"attributes": {"POSITION": 0}, "material": 1},
                             {"attributes": {"POSITION": 0}, "material": 2}]}],
  "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
  "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
  "nodes": [{"mesh": 0}],
  "scenes": [{"nodes": [0]}]
}"#;
    let model = read(&glb(json, &f32s(&SHEET))).unwrap();
    let parts = &model.nodes[0].parts;

    // 0.2 linear is 0.4845 displayed; a blend keeps its alpha and an opaque file loses it.
    assert!((parts[0].material.color[0] - 0.4845).abs() < 1e-3);
    assert_eq!(parts[0].material.name.as_deref(), Some("Mid"));
    assert_eq!(parts[1].material.color[3], 0.5);
    assert_eq!(parts[2].material.color[3], 1.0);
    assert_eq!(
        parts[0].bounds(),
        Some((Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0)))
    );
}

#[test]
fn reads_one_named_branch_out_of_a_hangar() {
    let json = r#"{
  "asset": {"version": "2.0"},
  "nodes": [
    {"name": "Tank", "children": [1]},
    {"name": "Turret", "children": [2]},
    {"name": "Gun"},
    {"name": "Jet", "children": [4]},
    {"name": "Wing"}
  ],
  "scenes": [{"nodes": [0, 3]}]
}"#;
    let file = glb(json, &[]);
    let whole = read(&file).unwrap();
    assert_eq!(whole.nodes.len(), 5);
    assert_eq!(whole.roots, vec![0, 3]);
    assert_eq!(whole.find("Turret"), Some(1));

    let turret = read_scene(&file, Some("Turret")).unwrap();
    assert_eq!(turret.nodes.len(), 2);
    assert_eq!(turret.roots, vec![0]);
    assert_eq!(turret.nodes[0].name.as_deref(), Some("Turret"));
    assert_eq!(turret.nodes[0].children, vec![1]);
    assert_eq!(turret.nodes[1].name.as_deref(), Some("Gun"));

    let jet = read_scene(&file, Some("Jet")).unwrap();
    assert_eq!(jet.nodes.len(), 2);
    assert_eq!(jet.find("Wing"), Some(1));

    let missing = read_scene(&file, Some("Submarine")).unwrap_err();
    assert!(matches!(missing, Error::NodeNotFound(_)));
}

#[test]
fn rejects_anything_that_is_not_a_glb() {
    assert!(!is_glb(b""), "an empty file is not a model");
    assert!(matches!(read(b"").unwrap_err(), Error::TooShort));
    assert!(matches!(read(b"# an obj file").unwrap_err(), Error::NotGlb));
    assert!(!is_glb(&[0u8; 4]));

    let json = r#"{"asset": {"version": "2.0"}, "nodes": [], "scenes": [{"nodes": []}]}"#;
    assert!(matches!(
        read(&glb_version(1, json, &[])).unwrap_err(),
        Error::Version(1)
    ));
}

#[test]
fn rejects_a_chunk_that_runs_past_the_end() {
    let file = glb(SHEET_JSON, &sheet());
    assert!(matches!(
        read(&file[..file.len() - 4]).unwrap_err(),
        Error::Truncated("file")
    ));

    // The position accessor claims four vertices the buffer only holds three of.
    let json = SHEET_JSON.replace(
        "\"count\": 3, \"type\": \"VEC3\"",
        "\"count\": 4, \"type\": \"VEC3\"",
    );
    assert!(matches!(
        read(&glb(&json, &sheet())).unwrap_err(),
        Error::Truncated("accessor")
    ));
}

#[test]
fn reads_a_chunk_length_that_leaves_out_the_padding() {
    let bin = sheet();
    let mut file = glb(SHEET_JSON, &bin);
    // A writer that declares the content it stored, not the padded bytes around it: the
    // container still has to find the next chunk.
    file[12..16].copy_from_slice(&(SHEET_JSON.len() as u32).to_le_bytes());
    let head = 12 + 8 + pad(SHEET_JSON.len());
    file[head..head + 4].copy_from_slice(&(bin.len() as u32).to_le_bytes());

    let model = read(&file).unwrap();
    assert_eq!(model.nodes[0].parts[0].positions.len(), 3);
    assert_eq!(model.triangle_count(), 1);
}

#[test]
fn rejects_a_document_that_is_not_gltf_two() {
    let json = r#"{"asset": {"version": "1.0"}, "nodes": []}"#;
    assert!(matches!(read(&glb(json, &[])).unwrap_err(), Error::Json(_)));

    let json = r#"{"nodes": []}"#;
    assert!(matches!(read(&glb(json, &[])).unwrap_err(), Error::Json(_)));

    let json = r#"[1, 2, 3]"#;
    assert!(matches!(read(&glb(json, &[])).unwrap_err(), Error::Json(_)));

    // A GLB with no JSON chunk at all, only an empty binary one.
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&20u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    assert!(matches!(read(&out).unwrap_err(), Error::MissingJson));
}

#[test]
fn refuses_features_it_does_not_read() {
    let json = SHEET_JSON.replace(
        "\"count\": 3, \"type\": \"VEC3\"",
        "\"count\": 3, \"type\": \"VEC3\", \"sparse\": {\"count\": 1}",
    );
    let why = read(&glb(&json, &sheet())).unwrap_err();
    assert!(matches!(why, Error::Sparse), "{why}");

    let json = r#"{"asset": {"version": "2.0"}, "extensionsRequired": ["KHR_materials_emissive"],
                  "nodes": []}"#;
    let why = read(&glb(json, &[])).unwrap_err();
    assert!(
        matches!(&why, Error::Requires(name) if name == "KHR_materials_emissive"),
        "{why}"
    );

    let json = SHEET_JSON.replace(
        "{\"attributes\": {\"POSITION\": 0}, \"indices\": 1}",
        "{\"attributes\": {\"POSITION\": 0}, \"indices\": 1, \"mode\": 1}",
    );
    let why = read(&glb(&json, &sheet())).unwrap_err();
    assert!(matches!(why, Error::Mode(1)), "{why}");

    let mut short = f32s(&SHEET);
    short.extend_from_slice(&u16s(&[0, 1, 9]));
    let why = read(&glb(SHEET_JSON, &short)).unwrap_err();
    assert!(
        matches!(
            why,
            Error::IndexOutOfRange {
                index: 9,
                vertices: 3
            }
        ),
        "{why}"
    );

    // A node pointing at a mesh that is not in the table.
    let json = SHEET_JSON.replace("\"mesh\": 0", "\"mesh\": 7");
    let why = read(&glb(&json, &sheet())).unwrap_err();
    assert!(matches!(why, Error::Dangling("meshes")), "{why}");

    // A file whose vertices live in another file.
    let json = r#"{"asset": {"version": "2.0"}, "buffers": [{"uri": "body.bin", "byteLength": 12}],
                  "nodes": []}"#;
    assert!(matches!(
        read(&glb(json, &[])).unwrap_err(),
        Error::Requires(_)
    ));

    // A file that wants its skin decoded.
    let json = r#"{"asset": {"version": "2.0"}, "images": [{}, {}], "nodes": []}"#;
    assert!(matches!(
        read(&glb(json, &[])).unwrap_err(),
        Error::Requires(_)
    ));
}

#[test]
fn writes_a_model_and_reads_the_same_model_back() {
    let mut model = Model::new(
        vec![
            Node {
                name: Some("Hull".into()),
                transform: Transform {
                    position: Vec3::new(1.0, 2.0, 3.0),
                    rotation: Quat::from_rotation_z(FRAC_PI_2),
                    scale: Vec3::new(1.0, 1.0, 2.0),
                },
                children: vec![1],
                parts: Vec::new(),
            },
            Node {
                name: Some("Turret".into()),
                transform: Transform::IDENTITY,
                children: Vec::new(),
                parts: vec![Part {
                    positions: vec![Vec3::ZERO, Vec3::X, Vec3::Z],
                    normals: vec![Vec3::NEG_Y; 3],
                    uvs: vec![Vec2::new(0.25, 0.75); 3],
                    indices: vec![0, 2, 1],
                    material: Material {
                        name: Some("Steel".into()),
                        color: [0.4, 0.5, 0.6, 1.0],
                    },
                }],
            },
        ],
        vec![0],
    );

    let bytes = write(&model);
    assert!(is_glb(&bytes), "what the writer makes is a GLB");
    let back = read(&bytes).unwrap();

    assert_eq!(back.roots, model.roots);
    assert_eq!(back.nodes.len(), model.nodes.len());
    for (before, after) in model.nodes.iter().zip(back.nodes.iter()) {
        assert_eq!(before.name, after.name);
        assert_eq!(before.children, after.children);
        assert_eq!(before.parts.len(), after.parts.len());
        assert!(
            same_place(before.transform, after.transform),
            "{} went from {:?} to {:?}",
            before.name.as_deref().unwrap_or("?"),
            before.transform,
            after.transform
        );
    }

    let before = &model.nodes[1].parts.remove(0);
    let after = &back.nodes[1].parts[0];
    assert_eq!(after.indices, vec![0, 2, 1]);
    for (before, after) in before.positions.iter().zip(after.positions.iter()) {
        assert!((before - after).length() < 1e-5, "{before:?} vs {after:?}");
    }
    for (before, after) in before.normals.iter().zip(after.normals.iter()) {
        assert!((before - after).length() < 1e-5, "{before:?} vs {after:?}");
    }
    assert!(
        after
            .uvs
            .iter()
            .all(|uv| (uv - Vec2::new(0.25, 0.75)).length() < 1e-5)
    );
    assert_eq!(after.material.name.as_deref(), Some("Steel"));
    for (before, after) in before
        .material
        .color
        .iter()
        .zip(after.material.color.iter())
    {
        assert!((before - after).abs() < 1e-4, "{before} vs {after}");
    }
}

/// Two nodes at the same place may state it with different numbers, so compare the matrices
/// they build rather than the position, rotation and scale they happen to write down.
fn same_place(a: Transform, b: Transform) -> bool {
    let (a, b) = (a.matrix().to_cols_array(), b.matrix().to_cols_array());
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-5)
}

#[test]
fn writes_an_empty_model_and_reads_it_back() {
    let model = Model::new(Vec::new(), Vec::new());
    let back = read(&write(&model)).unwrap();
    assert!(back.nodes.is_empty());
    assert!(back.roots.is_empty());
}
