use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use carbine_core::{NodeId, UiItem, UiText};

pub(super) struct Glyph {
    advance: f32,
    bounds: [f32; 4],
    uv: [f32; 4],
}

include!("../assets/font_metrics.rs");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

pub(super) struct Mesh {
    pub vertex: wgpu::Buffer,
    pub index: wgpu::Buffer,
    pub index_count: u32,
}

struct Cached {
    text: UiText,
    mesh: Option<Mesh>,
}

#[derive(Default)]
pub(super) struct Cache {
    pub atlas: Option<wgpu::TextureView>,
    entries: HashMap<NodeId, Cached>,
}

impl Cache {
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, items: &[UiItem<'_>]) {
        let live: HashSet<_> = items
            .iter()
            .filter(|item| item.text.is_some())
            .map(|item| item.node)
            .collect();
        self.entries.retain(|node, _| live.contains(node));
        if live.is_empty() {
            return;
        }
        self.atlas
            .get_or_insert_with(|| upload_atlas(device, queue));
        for item in items {
            let Some(text) = item.text else { continue };
            if self
                .entries
                .get(&item.node)
                .is_some_and(|cached| same_layout(&cached.text, text))
            {
                continue;
            }
            let (vertices, indices) = layout(text);
            let mut mesh = self
                .entries
                .remove(&item.node)
                .and_then(|cached| cached.mesh);
            if indices.is_empty() {
                mesh = None;
            } else {
                let vertex_bytes = bytemuck::cast_slice(&vertices);
                let index_bytes = bytemuck::cast_slice(&indices);
                if mesh.as_ref().is_none_or(|mesh| {
                    mesh.vertex.size() < vertex_bytes.len() as u64
                        || mesh.index.size() < index_bytes.len() as u64
                }) {
                    mesh = Some(Mesh {
                        vertex: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("carbine text vertices"),
                            size: (vertex_bytes.len() as u64).next_power_of_two(),
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        }),
                        index: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("carbine text indices"),
                            size: (index_bytes.len() as u64).next_power_of_two(),
                            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        }),
                        index_count: indices.len() as u32,
                    });
                }
                let mesh = mesh.as_mut().unwrap();
                mesh.index_count = indices.len() as u32;
                queue.write_buffer(&mesh.vertex, 0, vertex_bytes);
                queue.write_buffer(&mesh.index, 0, index_bytes);
            }
            self.entries.insert(
                item.node,
                Cached {
                    text: text.clone(),
                    mesh,
                },
            );
        }
    }

    pub fn mesh(&self, node: NodeId) -> Option<&Mesh> {
        self.entries.get(&node)?.mesh.as_ref()
    }
}

fn same_layout(a: &UiText, b: &UiText) -> bool {
    a.content == b.content
        && a.font_size == b.font_size
        && a.word_wrap == b.word_wrap
        && (!a.word_wrap || a.width == b.width)
}

fn glyph(character: char) -> &'static Glyph {
    let index = match character as u32 {
        code @ 32..=126 => code - 32,
        code @ 160..=255 => code - 160 + 95,
        _ => '?' as u32 - 32,
    };
    &GLYPHS[index as usize]
}

fn layout(text: &UiText) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let scale = text.font_size / FONT_SIZE;
    let line_height = LINE_HEIGHT * scale;
    let mut x = 0.0;
    let mut y = 0.0;
    for token in text.content.split_inclusive([' ', '\t', '\n', '\r']) {
        let word = token.trim_end_matches([' ', '\t', '\n', '\r']);
        let width: f32 = word.chars().map(|c| glyph(c).advance * scale).sum();
        if text.word_wrap && x > 0.0 && !word.is_empty() && x + width > text.width {
            x = 0.0;
            y += line_height;
        }
        for character in token.chars() {
            match character {
                '\n' => {
                    x = 0.0;
                    y += line_height;
                    continue;
                }
                '\r' => continue,
                '\t' => {
                    x += glyph(' ').advance * scale * 4.0;
                    continue;
                }
                _ => {}
            }
            let glyph = glyph(character);
            let advance = glyph.advance * scale;
            if text.word_wrap && character != ' ' && x > 0.0 && x + advance > text.width {
                x = 0.0;
                y += line_height;
            }
            if glyph.bounds != [0.0; 4] {
                let [left, top, right, bottom] = glyph.bounds;
                let [u0, v0, u1, v1] = glyph.uv;
                let base = vertices.len() as u32;
                for (px, py, u, v) in [
                    (left, top, u0, v0),
                    (right, top, u1, v0),
                    (right, bottom, u1, v1),
                    (left, bottom, u0, v1),
                ] {
                    vertices.push(Vertex {
                        position: [x + px * scale, y + py * scale, 0.0],
                        normal: [0.0, 0.0, 1.0],
                        uv: [u, v],
                    });
                }
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            x += advance;
        }
    }
    (vertices, indices)
}

fn upload_atlas(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: ATLAS_WIDTH,
        height: ATLAS_HEIGHT,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("carbine font atlas"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        include_bytes!("../assets/font.r8"),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(ATLAS_WIDTH),
            rows_per_image: Some(ATLAS_HEIGHT),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(content: &str) -> UiText {
        UiText {
            content: content.into(),
            font_size: FONT_SIZE,
            ..UiText::default()
        }
    }

    #[test]
    fn atlas_and_metrics_are_bounded() {
        assert_eq!(
            include_bytes!("../assets/font.r8").len(),
            (ATLAS_WIDTH * ATLAS_HEIGHT) as usize
        );
        for glyph in &GLYPHS {
            assert!(glyph.advance > 0.0);
            assert!(glyph.uv.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }

    #[test]
    fn empty_spaces_and_controls_draw_nothing() {
        assert!(layout(&text(" \t\r\n\u{a0}")).0.is_empty());
        assert!(layout(&text("")).1.is_empty());
    }

    #[test]
    fn glyphs_use_quads_and_scale_in_pixels() {
        let mut label = text("FPS 120");
        let (large, indices) = layout(&label);
        assert_eq!(indices.len(), 6 * 6);
        label.font_size /= 2.0;
        let (small, _) = layout(&label);
        for (a, b) in large.iter().zip(small) {
            assert_eq!(a.position.map(|n| n / 2.0), b.position);
            assert_eq!(a.uv, b.uv);
        }
    }

    #[test]
    fn wrap_breaks_at_words_and_then_inside_long_words() {
        let mut label = text("AA BB");
        label.word_wrap = true;
        label.width = glyph('A').advance * 2.0 + 1.0;
        let (vertices, _) = layout(&label);
        assert_eq!(vertices[8].position[1], glyph('B').bounds[1] + LINE_HEIGHT);
        label.content = "AAAA".into();
        let (vertices, _) = layout(&label);
        assert_eq!(vertices[8].position[1], glyph('A').bounds[1] + LINE_HEIGHT);
        label.word_wrap = false;
        let (vertices, _) = layout(&label);
        assert_eq!(vertices[8].position[1], glyph('A').bounds[1]);
    }

    #[test]
    fn newlines_tabs_and_unsupported_unicode_are_deterministic() {
        let (vertices, _) = layout(&text("A\r\nA\tA"));
        assert_eq!(
            vertices[4].position[1] - vertices[0].position[1],
            LINE_HEIGHT
        );
        assert_eq!(
            vertices[8].position[0] - vertices[4].position[0],
            glyph('A').advance + 4.0 * glyph(' ').advance
        );
        let (fallback, _) = layout(&text("\u{1f600}"));
        let (question, _) = layout(&text("?"));
        assert_eq!(fallback[0].uv, question[0].uv);
        assert_ne!(glyph('é').uv, glyph('?').uv);
    }

    #[test]
    fn changing_a_counter_reuses_buffers_and_destroying_it_releases_them() {
        let gpu = crate::Gpu::new(crate::create_instance(), None).unwrap();
        let mut scene = carbine_core::Scene::new();
        let node = scene.create_ui("Text", None, None).unwrap();
        scene.set_text_content(node, "FPS 120".into()).unwrap();
        let mut cache = Cache::default();
        cache.prepare(&gpu.device, &gpu.queue, &scene.ui_items());
        let atlas = cache.atlas.as_ref().unwrap().clone();
        let vertex = cache.mesh(node).unwrap().vertex.clone();
        scene.set_text_content(node, "FPS 99".into()).unwrap();
        cache.prepare(&gpu.device, &gpu.queue, &scene.ui_items());
        assert_eq!(&cache.mesh(node).unwrap().vertex, &vertex);
        assert_eq!(cache.mesh(node).unwrap().index_count, 30);
        assert_eq!(cache.atlas.as_ref(), Some(&atlas));
        scene.set_text_content(node, String::new()).unwrap();
        cache.prepare(&gpu.device, &gpu.queue, &scene.ui_items());
        assert!(cache.mesh(node).is_none());
        scene.destroy(node).unwrap();
        cache.prepare(&gpu.device, &gpu.queue, &scene.ui_items());
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn style_changes_do_not_relayout() {
        let a = text("FPS 120");
        let mut b = a.clone();
        b.outline = true;
        b.outline_width = 3.0;
        b.width = 10.0;
        assert!(same_layout(&a, &b));
        b.word_wrap = true;
        assert!(!same_layout(&a, &b));
        b = a.clone();
        b.content = "FPS 119".into();
        assert!(!same_layout(&a, &b));
    }
}
