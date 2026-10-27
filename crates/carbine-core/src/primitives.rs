use std::f32::consts::PI;

use glam::{Vec2, Vec3};

use crate::mesh::MeshData;

/// The kinds a world object can be built from. The world is z-up, so a
/// primitive's `size` is (length, width, height) along x, y and z.
pub const KINDS: &[&str] = &["cube", "plane", "sphere", "rectangle", "square"];

/// The shapes a UI element can take. Every one of them is the same quad: the UI
/// pass decides how it is filled, so `polygon` and `corner` turn a square into a
/// circle without a second mesh.
pub const UI_KINDS: &[&str] = &["square", "rectangle", "triangle", "hexagon", "circle"];

/// Geometry for a world kind, or `None` when the kind belongs to the UI overlay.
pub fn build(kind: &str) -> Option<MeshData> {
    match kind {
        "cube" => Some(cube()),
        "plane" => Some(plane()),
        "sphere" => Some(sphere(32, 16)),
        "rectangle" | "square" => Some(quad()),
        _ => None,
    }
}

/// Geometry for a UI kind: always the unit quad, sized and shaped later.
pub fn ui_mesh(kind: &str) -> Option<MeshData> {
    UI_KINDS.contains(&kind).then(quad)
}

pub fn default_name(kind: &str) -> String {
    let mut characters = kind.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => "Object".to_owned(),
    }
}

/// A one meter cube centred on the origin, so `size = (length, width, height)`
/// scales it directly. Each face carries its own normal and a full set of UVs.
fn cube() -> MeshData {
    // Normal, then four corners in the order that winds counter-clockwise as
    // seen from outside the face.
    const FACES: [(Vec3, [Vec3; 4]); 6] = [
        (
            Vec3::Z,
            [
                Vec3::new(-0.5, -0.5, 0.5),
                Vec3::new(0.5, -0.5, 0.5),
                Vec3::new(0.5, 0.5, 0.5),
                Vec3::new(-0.5, 0.5, 0.5),
            ],
        ),
        (
            Vec3::NEG_Z,
            [
                Vec3::new(-0.5, -0.5, -0.5),
                Vec3::new(-0.5, 0.5, -0.5),
                Vec3::new(0.5, 0.5, -0.5),
                Vec3::new(0.5, -0.5, -0.5),
            ],
        ),
        (
            Vec3::X,
            [
                Vec3::new(0.5, -0.5, -0.5),
                Vec3::new(0.5, 0.5, -0.5),
                Vec3::new(0.5, 0.5, 0.5),
                Vec3::new(0.5, -0.5, 0.5),
            ],
        ),
        (
            Vec3::NEG_X,
            [
                Vec3::new(-0.5, -0.5, -0.5),
                Vec3::new(-0.5, -0.5, 0.5),
                Vec3::new(-0.5, 0.5, 0.5),
                Vec3::new(-0.5, 0.5, -0.5),
            ],
        ),
        (
            Vec3::Y,
            [
                Vec3::new(-0.5, 0.5, -0.5),
                Vec3::new(-0.5, 0.5, 0.5),
                Vec3::new(0.5, 0.5, 0.5),
                Vec3::new(0.5, 0.5, -0.5),
            ],
        ),
        (
            Vec3::NEG_Y,
            [
                Vec3::new(-0.5, -0.5, -0.5),
                Vec3::new(0.5, -0.5, -0.5),
                Vec3::new(0.5, -0.5, 0.5),
                Vec3::new(-0.5, -0.5, 0.5),
            ],
        ),
    ];

    const CORNER_UVS: [Vec2; 4] = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];

    let mut mesh = MeshData {
        positions: Vec::with_capacity(24),
        normals: Vec::with_capacity(24),
        uvs: Vec::with_capacity(24),
        indices: Vec::with_capacity(36),
    };

    for (normal, corners) in FACES {
        let base = mesh.positions.len() as u32;
        for (index, corner) in corners.into_iter().enumerate() {
            mesh.positions.push(corner);
            mesh.normals.push(normal);
            mesh.uvs.push(CORNER_UVS[index]);
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    mesh
}

/// A one meter by one meter floor tile lying in the x/y ground plane, facing up
/// along +z. Its `size` has no height, which is why the third axis is a no-op.
fn plane() -> MeshData {
    quad()
}

/// A one meter sphere centred on the origin: `slices` around the equator and
/// `stacks` from the +z pole to the -z pole.
fn sphere(slices: u32, stacks: u32) -> MeshData {
    let (rings, columns) = (stacks + 1, slices + 1);
    let mut mesh = MeshData {
        positions: Vec::with_capacity((rings * columns) as usize),
        normals: Vec::with_capacity((rings * columns) as usize),
        uvs: Vec::with_capacity((rings * columns) as usize),
        indices: Vec::with_capacity((stacks * slices * 6) as usize),
    };

    for ring in 0..rings {
        // Measured from the top pole, so +z is up the way the world expects.
        let down = ring as f32 * PI / stacks as f32;
        let (out, up) = (down.sin(), down.cos());
        for column in 0..columns {
            let around = column as f32 * 2.0 * PI / slices as f32;
            let normal = Vec3::new(out * around.cos(), out * around.sin(), up);
            mesh.positions.push(normal * 0.5);
            mesh.normals.push(normal);
            // v grows upward, the same way the flat shapes are textured.
            mesh.uvs.push(Vec2::new(
                column as f32 / slices as f32,
                1.0 - ring as f32 / stacks as f32,
            ));
        }
    }

    for ring in 0..stacks {
        for column in 0..slices {
            let top = ring * columns + column;
            let bottom = top + columns;
            mesh.indices
                .extend_from_slice(&[top, bottom, bottom + 1, top, bottom + 1, top + 1]);
        }
    }

    mesh
}

/// A one meter by one meter quad in the x/y ground plane, facing +z. Every flat
/// Carbine shape is authored through `size`, so the mesh itself stays unit.
fn quad() -> MeshData {
    MeshData {
        positions: vec![
            Vec3::new(-0.5, -0.5, 0.0),
            Vec3::new(0.5, -0.5, 0.0),
            Vec3::new(0.5, 0.5, 0.0),
            Vec3::new(-0.5, 0.5, 0.0),
        ],
        normals: vec![Vec3::Z; 4],
        uvs: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_is_a_unit_quad() {
        let mesh = build("rectangle").unwrap();
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(mesh.natural_extent(), Vec3::new(1.0, 1.0, 1.0));

        let (min, max) = mesh.bounds().unwrap();
        assert_eq!(min, Vec3::new(-0.5, -0.5, 0.0));
        assert_eq!(max, Vec3::new(0.5, 0.5, 0.0));
    }

    #[test]
    fn a_square_is_the_same_geometry_as_a_rectangle() {
        let square = build("square").unwrap();
        let rectangle = build("rectangle").unwrap();

        assert_eq!(square.positions, rectangle.positions);
        assert_eq!(square.indices, rectangle.indices);
    }

    #[test]
    fn a_cube_is_one_metre_along_every_axis() {
        let mesh = build("cube").unwrap();

        assert_eq!(mesh.vertex_count(), 24);
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.natural_extent(), Vec3::ONE);
        assert_eq!(mesh.bounds().unwrap().0, Vec3::splat(-0.5));
        assert_eq!(mesh.bounds().unwrap().1, Vec3::splat(0.5));
        // One normal per face rather than per vertex, so a cube edge stays sharp.
        assert_eq!(mesh.normals[0], Vec3::Z);
        assert_eq!(mesh.normals[4], Vec3::NEG_Z);
        assert_eq!(mesh.normals[23], Vec3::NEG_Y);
    }

    #[test]
    fn a_sphere_is_one_metre_across_and_round_in_every_direction() {
        let mesh = build("sphere").unwrap();

        assert_eq!(mesh.natural_extent(), Vec3::ONE);
        assert_eq!(mesh.bounds().unwrap().0, Vec3::splat(-0.5));
        assert_eq!(mesh.bounds().unwrap().1, Vec3::splat(0.5));
        for position in &mesh.positions {
            assert!(
                (position.length() - 0.5).abs() < 1e-5,
                "{position} is off the shell"
            );
        }
        assert_eq!(mesh.normals[0], Vec3::Z, "the top pole points up");
    }

    #[test]
    fn a_plane_lies_flat_on_the_ground() {
        let mesh = build("plane").unwrap();

        assert!(
            mesh.positions.iter().all(|position| position.z == 0.0),
            "a plane should have no height"
        );
        assert_eq!(mesh.bounds().unwrap().0.z, 0.0);
    }

    #[test]
    fn every_triangle_winds_against_its_own_normal() {
        for kind in KINDS {
            let mesh = build(kind).unwrap();
            for triangle in mesh.indices.as_chunks::<3>().0 {
                let [a, b, c] = [
                    mesh.positions[triangle[0] as usize],
                    mesh.positions[triangle[1] as usize],
                    mesh.positions[triangle[2] as usize],
                ];
                let face = (b - a).cross(c - a);
                // The latitude rings meet the poles in duplicated vertices, which
                // leaves zero area triangles that carry no winding to check.
                if face.length_squared() < 1e-9 {
                    continue;
                }
                let outward = mesh.normals[triangle[0] as usize]
                    + mesh.normals[triangle[1] as usize]
                    + mesh.normals[triangle[2] as usize];
                assert!(
                    face.dot(outward) > 0.0,
                    "{kind} triangle {triangle:?} winds inward"
                );
            }
        }
    }

    #[test]
    fn ui_kinds_share_one_quad_and_world_kinds_do_not_accept_them() {
        for kind in UI_KINDS {
            let mesh = ui_mesh(kind).unwrap();
            assert_eq!(mesh.vertex_count(), 4, "{kind} should be the plain quad");
        }

        assert!(
            build("triangle").is_none(),
            "a world triangle is not a primitive"
        );
        assert!(ui_mesh("cube").is_none(), "a cube is not a UI shape");
        assert!(build("dodecahedron").is_none());
    }

    #[test]
    fn names_are_capitalized() {
        assert_eq!(default_name("cube"), "Cube");
        assert_eq!(default_name(""), "Object");
    }
}
