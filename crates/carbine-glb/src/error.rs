use std::fmt;

/// Every way a `.glb` can fail to be the file Carbine expects.
#[derive(Debug)]
pub enum Error {
    /// Fewer bytes than the 12 byte GLB header needs.
    TooShort,
    /// The first four bytes are not `glTF` — this is some other kind of file.
    NotGlb,
    /// A GLB that is not version 2.
    Version(u32),
    /// A chunk, accessor or buffer view that runs past the end of the file.
    Truncated(&'static str),
    /// A GLB with no JSON chunk in front.
    MissingJson,
    /// The JSON chunk is not readable, or is not shaped like a glTF.
    Json(String),
    /// A feature the file needs that Carbine does not implement. Named, so the message
    /// can say what to export instead.
    Requires(String),
    /// An accessor whose values live in a patch list rather than the buffer.
    Sparse,
    /// A `componentType` that is not one of the six glTF numeric types.
    ComponentType(u32),
    /// An accessor `type` that is not SCALAR/VEC2/VEC3/VEC4/MAT2/MAT3/MAT4.
    AccessorType(String),
    /// A primitive with no POSITION attribute, so there is nothing to draw.
    MissingPosition,
    /// A draw mode other than 4 (TRIANGLES). Strips, fans and points are not drawn.
    Mode(u32),
    /// An index that names a vertex the primitive does not have.
    IndexOutOfRange {
        index: u32,
        vertices: usize,
    },
    /// A `nodes`/`meshes`/`materials`/`accessors` reference outside its table.
    Dangling(&'static str),
    /// `read_scene` asked for a node name this file does not hold.
    NodeNotFound(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort => write!(f, "it is shorter than the 12 byte GLB header"),
            Error::NotGlb => write!(f, "it is not a GLB file"),
            Error::Version(v) => write!(f, "it is GLB version {v}; Carbine reads version 2"),
            Error::Truncated(what) => write!(f, "the {what} runs past the end of the file"),
            Error::MissingJson => write!(f, "it has no JSON chunk"),
            Error::Json(why) => write!(f, "its JSON chunk is unreadable: {why}"),
            Error::Requires(feature) => {
                write!(f, "it needs {feature}, which Carbine does not read")
            }
            Error::Sparse => write!(f, "it uses a sparse accessor"),
            Error::ComponentType(c) => write!(f, "component type {c} is not a glTF numeric type"),
            Error::AccessorType(t) => write!(f, "accessor type \"{t}\" is not one Carbine reads"),
            Error::MissingPosition => write!(f, "a primitive has no POSITION attribute"),
            Error::Mode(m) => write!(f, "draw mode {m} is not triangles"),
            Error::IndexOutOfRange { index, vertices } => write!(
                f,
                "an index names vertex {index} but the primitive only has {vertices}"
            ),
            Error::Dangling(table) => {
                write!(f, "it references an entry outside its {table} table")
            }
            Error::NodeNotFound(name) => write!(f, "no node is named \"{name}\""),
            Error::Io(why) => write!(f, "the file could not be read: {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(why: std::io::Error) -> Self {
        Error::Io(why)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
