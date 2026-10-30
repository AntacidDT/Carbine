//! The Packager: an authored file in, a Carbine-format file out.
//!
//! Objects come out `.glb`, audio `.ogg`, images `.ktx2`. The engine loads one format
//! per class — one parser, one point of failure — and a packed image is already the
//! bytes the GPU wants, so nothing decodes a picture while a scene is loading. This is
//! the same work Enginebay does when a file is dropped into a project; a script can do
//! it too, in real time, as `carbine.packager`.
//!
//! This crate is the only place in the workspace allowed to know what a PNG is. The
//! loader in `carbine_ktx` stays decoder-free so that a scene which merely names a skin
//! never drags forty codecs along with it.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum PackError {
    /// The file the script named is not a picture, or is not there.
    Source { path: String, why: String },
    /// A picture the BC7 encoder refused.
    Encoder { path: String, why: String },
    /// The packed bytes were ready and could not be written.
    Write { path: String, why: String },
}

impl fmt::Display for PackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source { path, why } => write!(
                formatter,
                "\"{path}\" could not be read as a picture: {why}. The Packager takes \
                 png, jpg, webp, tga and bmp."
            ),
            Self::Encoder { path, why } => write!(
                formatter,
                "\"{path}\" is a picture but would not compress to BC7: {why}"
            ),
            Self::Write { path, why } => {
                write!(
                    formatter,
                    "the packed image could not be written to \"{path}\": {why}"
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
    let picture = image::open(source).map_err(|error| PackError::Source {
        path: display(source),
        why: error.to_string(),
    })?;
    let pixels = picture.to_rgba8();
    let packed = carbine_ktx::pack(pixels.width(), pixels.height(), pixels.as_raw(), true)
        .map_err(|error| PackError::Encoder {
            path: display(source),
            why: error.to_string(),
        })?;

    let written = destination(source, target)?;
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
    std::fs::write(&written, packed).map_err(|error| PackError::Write {
        path: display(&written),
        why: error.to_string(),
    })?;
    Ok(written)
}

fn destination(source: &Path, target: Option<&Path>) -> Result<PathBuf, PackError> {
    match target {
        Some(directory) if names_a_directory(directory) => {
            std::fs::create_dir_all(directory).map_err(|error| PackError::Write {
                path: display(directory),
                why: error.to_string(),
            })?;
            Ok(directory.join(stem(source)).with_extension("ktx2"))
        }
        Some(path) => Ok(path.to_path_buf()),
        None => Ok(source.with_extension("ktx2")),
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
}
