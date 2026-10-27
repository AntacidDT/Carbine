use glam::{Vec2, Vec3};

use crate::mesh::MeshData;

pub const KINDS: &[&str] = &["rectangle", "square"];

pub fn build(kind: &str) -> Option<MeshData> {
    match kind {
        "rectangle" | "square" => Some(quad()),
        _ => None,
    }
}

pub fn default_name(kind: &str) -> String {
    let mut characters = kind.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => "Object".to_owned(),
    }
}

/// A one meter by one meter quad in the XY plane, facing +Z. Every flat
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
        assert_eq!(
            build("square").unwrap().positions,
            build("rectangle").unwrap().positions
        );
    }

    #[test]
    fn names_are_capitalized() {
        assert_eq!(default_name("rectangle"), "Rectangle");
        assert_eq!(default_name("mesh"), "Mesh");
        assert_eq!(default_name(""), "Object");
    }
}
