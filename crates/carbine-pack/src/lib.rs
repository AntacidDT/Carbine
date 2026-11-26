//! The Packager: an authored file in, a Carbine-format file out.
//!
//! Objects come out `.glb`, images `.ktx2`. The engine loads one format per class —
//! one parser, one point of failure — and a packed image is already the bytes the GPU
//! wants, so nothing decodes a picture while a scene is loading. This is the same work
//! Enginebay does when a file is dropped into a project; a script can do it too, in real
//! time, as `carbine.packager`.
//!
//! This crate is the only place in the workspace allowed to know what a PNG, an OBJ or
//! an STL is. The loaders stay decoder-free so a scene which merely names a skin or a
//! model never drags the converters along with it.

use std::ffi::OsStr;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};

#[derive(Debug, Clone)]
pub enum PackError {
    /// The file the script named is not an authored file of that kind, or is not there.
    Source {
        path: String,
        why: String,
        kind: &'static str,
        accepted: &'static str,
    },
    /// A picture the BC7 encoder refused.
    Encoder { path: String, why: String },
    /// The packed bytes were ready and could not be written.
    Write { path: String, why: String },
}

impl fmt::Display for PackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source {
                path,
                why,
                kind,
                accepted,
            } => write!(
                formatter,
                "\"{path}\" could not be read as {kind}: {why}. The Packager takes {accepted}."
            ),
            Self::Encoder { path, why } => write!(
                formatter,
                "\"{path}\" is a picture but would not compress to BC7: {why}"
            ),
            Self::Write { path, why } => {
                write!(
                    formatter,
                    "the packed file could not be written to \"{path}\": {why}"
                )
            }
        }
    }
}

impl std::error::Error for PackError {}

/// Converts one picture to a KTX2 file of BC7 blocks with a full mip chain, and returns
/// the path it wrote.
///
/// With no `target` the file lands next to its source under the same stem, so
/// `wood.png` becomes `wood.ktx2` and the path a script hands to
/// `obj.texture` is the one it can already see in its project folder. A `target` that
/// names a folder — one that exists, or one ending in a separator — does the same
/// inside that folder; anything else is the file to write.
pub fn image(source: &Path, target: Option<&Path>) -> Result<PathBuf, PackError> {
    let picture = image::open(source).map_err(|error| source_error(source, error.to_string()))?;
    let pixels = picture.to_rgba8();
    let packed = carbine_ktx::pack(pixels.width(), pixels.height(), pixels.as_raw(), true)
        .map_err(|error| PackError::Encoder {
            path: display(source),
            why: error.to_string(),
        })?;
    write_packed(source, target, "ktx2", packed)
}

/// Converts an authored `.obj` or `.stl` into the one model format the engine reads,
/// returning the path of the `.glb` it wrote.
///
/// The destination follows the same rules as [`image`]: beside the source under the
/// same stem unless `target` names a folder or an exact file. The result goes straight
/// to an object's `model`, where the file's nodes come in as children.
///
/// OBJ and STL are read as Z-up, the way Carbine lays out a world; the writer crosses
/// them into glTF's Y-up and the runtime reader crosses them back, so a vertex in the
/// authored file stands where it stood before conversion. Materials become flat
/// colours — skins, animation and morph targets have nowhere to live in a Carbine GLB.
pub fn model(source: &Path, target: Option<&Path>) -> Result<PathBuf, PackError> {
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let converted = match extension.as_str() {
        "obj" => obj_model(source)?,
        "stl" => stl_model(source)?,
        _ => {
            return Err(model_error(
                source,
                "only .obj and .stl are models the Packager reads".to_owned(),
            ));
        }
    };
    if converted.vertex_count() == 0 {
        return Err(source_error(source, "it holds no triangles".to_owned()));
    }
    write_packed(source, target, "glb", carbine_glb::write(&converted))
}

/// An OBJ becomes one node per authored object, with one part each. Faces are
/// triangulated with their own vertices so a face normal or a missing normal can never
/// be smeared across a neighbouring face that needs a different one.
fn obj_model(source: &Path) -> Result<carbine_glb::Model, PackError> {
    let (objects, materials) = tobj::load_obj(source, &tobj::GPU_LOAD_OPTIONS)
        .map_err(|error| model_error(source, format!("the OBJ parser refused it: {error:?}")))?;
    // A missing MTL is not a failed model: the file still has its triangles, and a
    // flat default colour is better than refusing a shape because its palette is gone.
    let materials = materials.unwrap_or_default();

    let mut nodes = Vec::with_capacity(objects.len());
    for object in &objects {
        let authored = &object.mesh;
        if authored.positions.len() % 3 != 0 {
            return Err(model_error(
                source,
                "its positions are not whole vertices".to_owned(),
            ));
        }
        if authored.indices.len() % 3 != 0 {
            return Err(model_error(
                source,
                "its faces are not whole triangles".to_owned(),
            ));
        }

        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        for triangle in authored.indices.as_chunks::<3>().0 {
            let corners: Vec<Vec3> = triangle
                .iter()
                .map(|index| vertex_at(source, &authored.positions, *index))
                .collect::<Result<_, _>>()?;
            let face_normal = plane_normal(&corners);

            for (corner, index) in triangle.iter().enumerate() {
                let at = *index as usize;
                let normal = if authored.normals.len() >= at * 3 + 3 {
                    Vec3::new(
                        authored.normals[at * 3],
                        authored.normals[at * 3 + 1],
                        authored.normals[at * 3 + 2],
                    )
                } else {
                    face_normal
                };
                let uv = if authored.texcoords.len() >= at * 2 + 2 {
                    Vec2::new(authored.texcoords[at * 2], authored.texcoords[at * 2 + 1])
                } else {
                    Vec2::ZERO
                };
                positions.push(corners[corner]);
                normals.push(normal);
                uvs.push(uv);
                indices.push(indices.len() as u32);
            }
        }

        let material = authored
            .material_id
            .and_then(|id| materials.get(id))
            .map(|source| {
                let [r, g, b] = source.diffuse.unwrap_or([1.0, 1.0, 1.0]);
                carbine_glb::Material {
                    name: Some(source.name.clone()),
                    color: [
                        r.clamp(0.0, 1.0),
                        g.clamp(0.0, 1.0),
                        b.clamp(0.0, 1.0),
                        source.dissolve.unwrap_or(1.0).clamp(0.0, 1.0),
                    ],
                }
            })
            .unwrap_or(carbine_glb::Material::DEFAULT);

        nodes.push(carbine_glb::Node {
            name: (!object.name.is_empty()).then(|| object.name.clone()),
            transform: carbine_glb::Transform::IDENTITY,
            children: Vec::new(),
            parts: vec![carbine_glb::Part {
                positions,
                normals,
                uvs,
                indices,
                material,
            }],
        });
    }

    let roots = (0..nodes.len()).collect();
    Ok(carbine_glb::Model::new(nodes, roots))
}

/// An STL is already triangles. Each face gets its own three vertices, because its
/// flat normal belongs to the face and not to a vertex the next face may share.
fn stl_model(source: &Path) -> Result<carbine_glb::Model, PackError> {
    let file = File::open(source).map_err(|error| source_error(source, error.to_string()))?;
    let mut reader = BufReader::new(file);
    let authored =
        stl_io::read_stl(&mut reader).map_err(|error| model_error(source, error.to_string()))?;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    for face in &authored.faces {
        let corners: Vec<Vec3> = face
            .vertices
            .iter()
            .map(|index| {
                authored
                    .vertices
                    .get(*index)
                    .map(|vertex| Vec3::new(vertex[0], vertex[1], vertex[2]))
                    .ok_or_else(|| {
                        model_error(source, "a face points outside its vertex list".to_owned())
                    })
            })
            .collect::<Result<_, _>>()?;
        let raw: [f32; 3] = face.normal.into();
        let normal = Vec3::from(raw);
        let normal = if normal.length_squared() < 1e-12 {
            plane_normal(&corners)
        } else {
            normal.normalize()
        };
        for corner in corners {
            positions.push(corner);
            normals.push(normal);
            uvs.push(Vec2::ZERO);
            indices.push(indices.len() as u32);
        }
    }

    let name = source
        .file_stem()
        .unwrap_or_else(|| OsStr::new("model"))
        .to_string_lossy()
        .into_owned();
    Ok(carbine_glb::Model::new(
        vec![carbine_glb::Node {
            name: Some(name),
            transform: carbine_glb::Transform::IDENTITY,
            children: Vec::new(),
            parts: vec![carbine_glb::Part {
                positions,
                normals,
                uvs,
                indices,
                material: carbine_glb::Material::DEFAULT,
            }],
        }],
        vec![0],
    ))
}

fn vertex_at(source: &Path, positions: &[f32], index: u32) -> Result<Vec3, PackError> {
    let at = index as usize * 3;
    positions
        .get(at..at + 3)
        .map(|point| Vec3::new(point[0], point[1], point[2]))
        .ok_or_else(|| model_error(source, "a face points outside its vertex list".to_owned()))
}

fn plane_normal(corners: &[Vec3]) -> Vec3 {
    let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
    if normal.length_squared() < 1e-12 {
        Vec3::Z
    } else {
        normal.normalize()
    }
}

fn source_error(source: &Path, why: String) -> PackError {
    PackError::Source {
        path: display(source),
        why,
        kind: "a picture",
        accepted: "png, jpg, webp, tga and bmp",
    }
}

fn model_error(source: &Path, why: String) -> PackError {
    PackError::Source {
        path: display(source),
        why,
        kind: "a model",
        accepted: "obj and stl",
    }
}

fn write_packed(
    source: &Path,
    target: Option<&Path>,
    extension: &str,
    bytes: Vec<u8>,
) -> Result<PathBuf, PackError> {
    let written = destination(source, target, extension)?;
    // A bare file name has no directory to make, and `create_dir_all("")` is an error.
    if let Some(directory) = written
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
    {
        std::fs::create_dir_all(directory).map_err(|error| PackError::Write {
            path: display(&written),
            why: error.to_string(),
        })?;
    }
    std::fs::write(&written, bytes).map_err(|error| PackError::Write {
        path: display(&written),
        why: error.to_string(),
    })?;
    Ok(written)
}

fn destination(
    source: &Path,
    target: Option<&Path>,
    extension: &str,
) -> Result<PathBuf, PackError> {
    match target {
        Some(directory) if names_a_directory(directory) => {
            std::fs::create_dir_all(directory).map_err(|error| PackError::Write {
                path: display(directory),
                why: error.to_string(),
            })?;
            Ok(directory.join(stem(source)).with_extension(extension))
        }
        Some(path) => Ok(path.to_path_buf()),
        None => Ok(source.with_extension(extension)),
    }
}

/// Whether the caller means a folder rather than a file: one that already exists, or
/// one whose name ends in a separator. Anything else is the exact path to write.
fn names_a_directory(path: &Path) -> bool {
    let text = path.to_string_lossy();
    path.is_dir() || text.ends_with('/') || text.ends_with('\\')
}

fn stem(source: &Path) -> &OsStr {
    source.file_stem().unwrap_or_else(|| OsStr::new("image"))
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of its own, so a rerun never reads a stale file.
    fn workspace(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("carbine-pack-{name}"));
        std::fs::remove_dir_all(&directory).ok();
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn png(directory: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = directory.join(format!("{name}.png"));
        image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 5) as u8, 128, 255])
        })
        .save(&path)
        .unwrap();
        path
    }

    const OBJ: &str = "o Triangle\nv 0 0 0\nv 2 0 0\nv 0 3 0\nf 1 2 3\n";
    const STL: &str = "solid slab\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 2 0 0\nvertex 0 3 0\nendloop\nendfacet\nendsolid slab\n";

    #[test]
    fn a_picture_lands_next_to_its_source_under_the_same_stem() {
        let directory = workspace("sibling");
        let source = png(&directory, "wood", 16, 8);

        let written = image(&source, None).unwrap();

        assert_eq!(written, directory.join("wood.ktx2"));
        assert!(written.is_file());
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_folder_target_keeps_the_pictures_own_stem() {
        let directory = workspace("target");
        let source = png(&directory, "brick", 8, 8);

        let built = directory.join("built");
        std::fs::create_dir_all(&built).unwrap();
        assert_eq!(
            image(&source, Some(&built)).unwrap(),
            built.join("brick.ktx2")
        );
        let named = directory.join("flooring.ktx2");
        assert_eq!(image(&source, Some(named.as_path())).unwrap(), named);

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_trailing_separator_asks_for_a_folder() {
        let directory = workspace("separator");
        let source = png(&directory, "brick", 8, 8);
        let built = format!(
            "{}{}",
            directory.join("built").display(),
            std::path::MAIN_SEPARATOR
        );

        assert_eq!(
            image(&source, Some(Path::new(&built))).unwrap(),
            directory.join("built").join("brick.ktx2")
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn the_written_file_is_a_packed_image_the_loader_reads_back() {
        let directory = workspace("roundtrip");
        let source = png(&directory, "crate", 32, 16);

        let bytes = std::fs::read(image(&source, None).unwrap()).unwrap();
        let texture = carbine_ktx::read(&bytes).unwrap();

        assert_eq!((texture.width(), texture.height()), (32, 16));
        assert!(texture.srgb(), "an authored picture is display-encoded");
        assert_eq!(texture.levels().len(), 6);
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_picture_that_is_not_one_is_named_in_the_refusal() {
        let directory = workspace("refusal");
        let missing = directory.join("ghost.png");
        let error = image(&missing, None).unwrap_err();
        assert!(error.to_string().contains("ghost.png"), "{error}");

        let text = directory.join("notes.txt");
        std::fs::write(&text, "not a picture").unwrap();
        assert!(matches!(image(&text, None), Err(PackError::Source { .. })));
        std::fs::remove_dir_all(&directory).ok();
    }

    fn authored_model(directory: &Path, name: &str, text: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn an_obj_becomes_a_glb_the_runtime_reader_walks() {
        let directory = workspace("obj");
        let source = authored_model(&directory, "triangle.obj", OBJ);

        let written = model(&source, None).unwrap();
        assert_eq!(written, directory.join("triangle.glb"));
        let bytes = std::fs::read(&written).unwrap();
        let read = carbine_glb::read(&bytes).unwrap();

        assert_eq!(read.nodes.len(), 1);
        assert_eq!(read.nodes[0].name.as_deref(), Some("Triangle"));
        assert_eq!(read.vertex_count(), 3);
        assert_eq!(read.triangle_count(), 1);
        let part = &read.nodes[0].parts[0];
        assert!((part.positions[0] - Vec3::ZERO).length() < 1e-6);
        assert!((part.positions[1] - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-6);
        assert!((part.positions[2] - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-6);

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn an_stl_becomes_a_glb_with_its_flat_normal() {
        let directory = workspace("stl");
        let source = authored_model(&directory, "slab.stl", STL);

        let written = model(&source, None).unwrap();
        let read = carbine_glb::read(&std::fs::read(written).unwrap()).unwrap();

        assert_eq!(read.nodes.len(), 1);
        assert_eq!(read.nodes[0].name.as_deref(), Some("slab"));
        assert_eq!(read.triangle_count(), 1);
        assert!((read.nodes[0].parts[0].normals[0] - Vec3::Z).length() < 1e-6);

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_model_can_be_written_beside_its_source_or_into_a_folder() {
        let directory = workspace("model-target");
        let source = authored_model(&directory, "chair.obj", OBJ);
        let built = directory.join("built");
        std::fs::create_dir_all(&built).unwrap();

        assert_eq!(
            model(&source, Some(&built)).unwrap(),
            built.join("chair.glb")
        );
        let named = directory.join("seat.glb");
        assert_eq!(model(&source, Some(&named)).unwrap(), named);
        assert!(named.is_file());

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_file_that_is_not_a_model_is_named_in_the_refusal() {
        let directory = workspace("model-refusal");
        let missing = directory.join("ghost.stl");
        let error = model(&missing, None).unwrap_err();
        assert!(error.to_string().contains("ghost.stl"), "{error}");

        let text = authored_model(&directory, "notes.txt", "words, not triangles");
        let error = model(&text, None).unwrap_err();
        assert!(error.to_string().contains("obj and stl"), "{error}");
        assert!(matches!(model(&text, None), Err(PackError::Source { .. })));

        std::fs::remove_dir_all(&directory).ok();
    }
}
