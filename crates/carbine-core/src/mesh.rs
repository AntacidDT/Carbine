use glam::{Vec2, Vec3};

/// Identity used by the renderer to cache one GPU vertex buffer per mesh.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MeshHandle(pub u64);

#[derive(Clone, Debug)]
pub struct MeshData {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

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

    /// The mesh's own dimensions in meters, used to turn a requested `size`
    /// into a scale. Axes with no extent keep a factor of one so that flat
    /// shapes stay authorable in their remaining axes.
    pub fn natural_extent(&self) -> Vec3 {
        match self.bounds() {
            Some((min, max)) => {
                let extent = max - min;
                Vec3::new(
                    if extent.x > f32::EPSILON {
                        extent.x
                    } else {
                        1.0
                    },
                    if extent.y > f32::EPSILON {
                        extent.y
                    } else {
                        1.0
                    },
                    if extent.z > f32::EPSILON {
                        extent.z
                    } else {
                        1.0
                    },
                )
            }
            None => Vec3::ONE,
        }
    }
}
