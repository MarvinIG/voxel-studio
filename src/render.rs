use crate::model::{Mesh, Volume, DIRECTIONS};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use glam::Vec3;

#[derive(Clone)]
pub struct Face {
    pub points: Vec<Vec3>,
    pub color: [u8; 3],
    pub normal: Vec3,
    pub voxel: Option<[i32; 3]>,
}
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub pan: Vec2,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.7,
            pitch: 0.48,
            zoom: 1.,
            pan: Vec2::ZERO,
        }
    }
}
impl Camera {
    fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let direction = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        let right = Vec3::new(self.yaw.cos(), 0., -self.yaw.sin());
        (right, direction.cross(right), direction)
    }
    pub fn project(&self, p: Vec3, rect: Rect) -> (Pos2, f32) {
        let (right, up, dir) = self.basis();
        let scale = rect.width().min(rect.height()) * 0.65 * self.zoom;
        (
            rect.center() + self.pan + Vec2::new(p.dot(right), -p.dot(up)) * scale,
            p.dot(dir),
        )
    }
}
pub fn volume_faces(volume: &Volume) -> Vec<Face> {
    let mut faces = vec![];
    let s = volume.size as f32;
    let half = 0.5 / s;
    for (&p, &color) in &volume.cells {
        let center = Vec3::new(p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5) / s
            - Vec3::splat(0.5);
        for d in DIRECTIONS {
            let q = std::array::from_fn(|i| p[i] + d[i]);
            if volume.cells.contains_key(&q) {
                continue;
            }
            let n = Vec3::new(d[0] as f32, d[1] as f32, d[2] as f32);
            let u = if d[1] != 0 { Vec3::X } else { Vec3::Y };
            let v = n.cross(u);
            let points = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
                .map(|(a, b)| center + (n + u * a + v * b) * half)
                .to_vec();
            faces.push(Face {
                points,
                color,
                normal: n,
                voxel: Some(p),
            });
        }
    }
    faces
}
pub fn mesh_faces(mesh: &Mesh) -> Vec<Face> {
    let mut faces = Vec::new();
    for t in &mesh.triangles {
        let normal = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]).normalize_or_zero();
        let n = if mesh.materials[t.material].texture.is_some() && mesh.triangles.len() < 20_000 {
            8
        } else {
            1
        };
        let bary = |i: usize, j: usize| {
            Vec3::new(
                1. - (i + j) as f32 / n as f32,
                i as f32 / n as f32,
                j as f32 / n as f32,
            )
        };
        let mut add = |b: [Vec3; 3]| {
            faces.push(Face {
                points: b
                    .map(|v| t.p[0] * v.x + t.p[1] * v.y + t.p[2] * v.z)
                    .to_vec(),
                color: mesh.color(t, (b[0] + b[1] + b[2]) / 3.),
                normal,
                voxel: None,
            });
        };
        for i in 0..n {
            for j in 0..n - i {
                add([bary(i, j), bary(i + 1, j), bary(i, j + 1)]);
                if i + j + 1 < n {
                    add([bary(i + 1, j), bary(i + 1, j + 1), bary(i, j + 1)]);
                }
            }
        }
    }
    faces
}
#[derive(Clone)]
pub struct Hit {
    pub voxel: [i32; 3],
    pub normal: [i32; 3],
}
pub fn draw(
    ui: &mut egui::Ui,
    rect: Rect,
    camera: &Camera,
    faces: &[Face],
    grid: bool,
    outlines: bool,
    pointer: Option<Pos2>,
) -> Option<Hit> {
    let painter = ui.painter_at(rect);
    if grid {
        for i in -8..=8 {
            let t = i as f32 / 8.;
            let color = if i == 0 {
                Color32::from_rgb(183, 179, 195)
            } else {
                Color32::from_rgb(218, 215, 225)
            };
            let a = camera.project(Vec3::new(t, -0.505, -1.), rect).0;
            let b = camera.project(Vec3::new(t, -0.505, 1.), rect).0;
            let c = camera.project(Vec3::new(-1., -0.505, t), rect).0;
            let d = camera.project(Vec3::new(1., -0.505, t), rect).0;
            painter.line_segment([a, b], Stroke::new(0.65f32, color));
            painter.line_segment([c, d], Stroke::new(0.65f32, color));
        }
        let shadow_center = camera.project(Vec3::new(0., -0.5, 0.), rect).0;
        for i in (0..6).rev() {
            let r = (0.23 + i as f32 * 0.025) * rect.height() * camera.zoom;
            painter.add(egui::Shape::ellipse_filled(
                shadow_center,
                Vec2::new(r, r * 0.25),
                Color32::from_black_alpha(4),
            ));
        }
    }
    let (_, _, dir) = camera.basis();
    let mut sorted: Vec<_> = faces
        .iter()
        .filter(|f| f.voxel.is_none() || f.normal.dot(dir) > 0.001)
        .map(|f| {
            let depth = f.points.iter().map(|p| p.dot(dir)).sum::<f32>() / f.points.len() as f32;
            (f, depth)
        })
        .collect();
    sorted.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
    let light = Vec3::new(-0.3, 0.85, 0.45).normalize();
    let mut hit = None;
    for (f, _) in sorted {
        let points: Vec<_> = f
            .points
            .iter()
            .map(|&p| camera.project(p, rect).0)
            .collect();
        let brightness = 0.68 + 0.32 * f.normal.dot(light).max(0.);
        let color = Color32::from_rgb(
            (f.color[0] as f32 * brightness) as u8,
            (f.color[1] as f32 * brightness) as u8,
            (f.color[2] as f32 * brightness) as u8,
        );
        let mut polygon = egui::Mesh::default();
        for &p in &points {
            polygon.colored_vertex(p, color);
        }
        for i in 1..points.len() - 1 {
            polygon.add_triangle(0, i as u32, (i + 1) as u32);
        }
        painter.add(egui::Shape::mesh(polygon));
        if outlines {
            painter.add(egui::Shape::closed_line(
                points.clone(),
                Stroke::new(0.4f32, Color32::from_black_alpha(35)),
            ));
        }
        if let (Some(mouse), Some(voxel)) = (pointer, f.voxel) {
            if inside(mouse, &points) {
                hit = Some(Hit {
                    voxel,
                    normal: [f.normal.x as i32, f.normal.y as i32, f.normal.z as i32],
                });
            }
        }
    }
    if let Some(h) = &hit {
        if let Some(f) = faces.iter().find(|f| {
            f.voxel == Some(h.voxel)
                && f.normal == Vec3::new(h.normal[0] as f32, h.normal[1] as f32, h.normal[2] as f32)
        }) {
            painter.add(egui::Shape::closed_line(
                f.points
                    .iter()
                    .map(|&p| camera.project(p, rect).0)
                    .collect(),
                Stroke::new(2.0f32, Color32::from_rgb(153, 111, 239)),
            ));
        }
    }
    if grid {
        let origin = rect.right_bottom() - Vec2::new(48., 48.);
        let (right, up, _) = camera.basis();
        for (axis, label, color) in [
            (Vec3::X, "X", Color32::from_rgb(207, 104, 117)),
            (Vec3::Y, "Y", Color32::from_rgb(108, 158, 128)),
            (Vec3::Z, "Z", Color32::from_rgb(110, 139, 199)),
        ] {
            let end = origin + Vec2::new(axis.dot(right), -axis.dot(up)) * 25.;
            painter.line_segment([origin, end], Stroke::new(2.0f32, color));
            painter.text(
                end,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::monospace(11.),
                color,
            );
        }
    }
    hit
}
fn inside(p: Pos2, poly: &[Pos2]) -> bool {
    let mut sign = 0f32;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let c = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if c.abs() > 0.01 {
            if sign != 0. && c.signum() != sign {
                return false;
            }
            sign = c.signum();
        }
    }
    true
}
