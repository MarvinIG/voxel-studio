use glam::{Vec2, Vec3};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    process::Command,
};

#[derive(Clone)]
pub struct Triangle {
    pub p: [Vec3; 3],
    pub uv: [Vec2; 3],
    pub material: usize,
}
#[derive(Clone)]
pub struct Material {
    pub color: [u8; 3],
    pub texture: Option<image::RgbaImage>,
}
#[derive(Clone)]
pub struct Mesh {
    pub triangles: Vec<Triangle>,
    pub materials: Vec<Material>,
    pub warnings: Vec<String>,
}
#[derive(Clone, PartialEq, Debug)]
pub struct Volume {
    pub size: u32,
    pub cells: HashMap<[i32; 3], [u8; 3]>,
}

impl Mesh {
    pub fn load(path: &Path) -> Result<Self, String> {
        match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "obj" => Self::obj(path),
            "blend" => {
                let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
                let script = temp.path().join("export.py");
                std::fs::write(&script, include_str!("../scripts/blend_export.py"))
                    .map_err(|e| e.to_string())?;
                let out = temp.path().join("model.obj");
                let blender = std::env::var("BLENDER_PATH").unwrap_or_else(|_| "blender".into());
                let result = Command::new(blender).args(["--background", "--disable-autoexec"]).arg(path)
                    .arg("--python").arg(script).arg("--").arg(&out).output()
                    .map_err(|e| format!("Blender konnte nicht gestartet werden: {e}. Blender installieren oder BLENDER_PATH setzen."))?;
                if !result.status.success() || !out.exists() {
                    return Err(format!(
                        "Blender-Import fehlgeschlagen: {}",
                        String::from_utf8_lossy(&result.stderr)
                    ));
                }
                Self::obj(&out)
            }
            _ => Err("Bitte eine .obj- oder .blend-Datei auswählen.".into()),
        }
    }
    fn obj(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let absolute = path.canonicalize().map_err(|e| e.to_string())?;
        let base = absolute.parent().unwrap_or(Path::new("."));
        let (objects, materials) = tobj::load_obj_buf(
            &mut std::io::BufReader::new(file),
            &tobj::LoadOptions {
                triangulate: true,
                single_index: true,
                ..Default::default()
            },
            |mtl_path| {
                let full = base.join(mtl_path);
                let (mut materials, map) = tobj::load_mtl(&full)?;
                for material in &mut materials {
                    if let Some(texture) = &mut material.diffuse_texture {
                        let resolved = full
                            .parent()
                            .unwrap_or(base)
                            .join(texture.replace('\\', "/"));
                        // Resolve now: image paths are relative to the MTL, not the OBJ.
                        if let Ok(absolute) = resolved.canonicalize() {
                            *texture = absolute.to_string_lossy().into_owned();
                        } else {
                            *texture = resolved.to_string_lossy().into_owned();
                        }
                    }
                }
                Ok((materials, map))
            },
        )
        .map_err(|e| e.to_string())?;
        let mut warnings = vec![];
        let mats = materials.unwrap_or_else(|e| {
            warnings.push(format!("Materialdatei: {e}"));
            vec![]
        });
        let mut materials: Vec<_> = mats
            .iter()
            .map(|m| {
                let c = m.diffuse.unwrap_or([0.65, 0.72, 0.8]);
                let texture = m
                    .diffuse_texture
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .and_then(|name| {
                        let p = path
                            .parent()
                            .unwrap_or(Path::new("."))
                            .join(name.replace('\\', "/"));
                        match image::open(&p) {
                            Ok(i) => Some(i.to_rgba8()),
                            Err(e) => {
                                warnings.push(format!("Textur {}: {e}", p.display()));
                                None
                            }
                        }
                    });
                Material {
                    color: c.map(|x| (x.clamp(0., 1.) * 255.) as u8),
                    texture,
                }
            })
            .collect();
        let fallback = materials.len();
        materials.push(Material {
            color: [161, 181, 199],
            texture: None,
        });
        let mut triangles = vec![];
        for object in objects {
            let m = object.mesh;
            for f in m.indices.as_chunks::<3>().0 {
                let p = std::array::from_fn(|i| {
                    let n = f[i] as usize * 3;
                    Vec3::from_slice(&m.positions[n..n + 3])
                });
                if p.iter().any(|p| !p.is_finite()) {
                    return Err("Das Modell enthält ungültige Koordinaten.".into());
                }
                let uv = std::array::from_fn(|i| {
                    let n = f[i] as usize * 2;
                    if n + 1 < m.texcoords.len() {
                        Vec2::new(m.texcoords[n], m.texcoords[n + 1])
                    } else {
                        Vec2::ZERO
                    }
                });
                triangles.push(Triangle {
                    p,
                    uv,
                    material: m.material_id.filter(|&i| i < fallback).unwrap_or(fallback),
                });
            }
        }
        if triangles.is_empty() {
            return Err("Das Modell enthält keine Dreiecke.".into());
        }
        let min = triangles
            .iter()
            .flat_map(|t| t.p)
            .fold(Vec3::splat(f32::INFINITY), Vec3::min);
        let max = triangles
            .iter()
            .flat_map(|t| t.p)
            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
        let extent = (max - min).max_element();
        if extent < 1e-8 {
            return Err("Das Modell hat keine räumliche Ausdehnung.".into());
        }
        let center = (min + max) * 0.5;
        for t in &mut triangles {
            for p in &mut t.p {
                *p = (*p - center) / extent;
            }
        }
        Ok(Self {
            triangles,
            materials,
            warnings,
        })
    }
    pub fn color(&self, t: &Triangle, bary: Vec3) -> [u8; 3] {
        let m = &self.materials[t.material];
        if let Some(tex) = &m.texture {
            let uv = t.uv[0] * bary.x + t.uv[1] * bary.y + t.uv[2] * bary.z;
            let x = (uv.x.rem_euclid(1.) * tex.width() as f32) as u32;
            let y = ((1. - uv.y.rem_euclid(1.)) * tex.height() as f32) as u32;
            let p = tex.get_pixel(x.min(tex.width() - 1), y.min(tex.height() - 1));
            [p[0], p[1], p[2]]
        } else {
            m.color
        }
    }
    pub fn voxelize(&self, size: u32, solid: bool, textures: bool) -> Volume {
        let size = size.clamp(4, 128);
        let scale = (size - 1) as f32;
        let mut cells = HashMap::new();
        for t in &self.triangles {
            let p = t.p.map(|v| (v + Vec3::splat(0.5)) * scale);
            let min = p
                .into_iter()
                .fold(Vec3::splat(f32::INFINITY), Vec3::min)
                .floor()
                .as_ivec3()
                .max(glam::IVec3::ZERO);
            let max = p
                .into_iter()
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max)
                .ceil()
                .as_ivec3()
                .min(glam::IVec3::splat(size as i32 - 1));
            for z in min.z..=max.z {
                for y in min.y..=max.y {
                    for x in min.x..=max.x {
                        let c = Vec3::new(x as f32, y as f32, z as f32);
                        if triangle_box(p, c) {
                            let bary = closest_barycentric(p, c);
                            let color = if textures {
                                self.color(t, bary)
                            } else {
                                self.materials[t.material].color
                            };
                            cells.insert([x, y, z], color);
                        }
                    }
                }
            }
        }
        if solid {
            fill(&mut cells, size);
        }
        Volume { size, cells }
    }
}
// Separating axis theorem: three box axes, triangle normal, nine edge cross axes.
fn triangle_box(p: [Vec3; 3], center: Vec3) -> bool {
    let v = p.map(|p| p - center);
    let edges = [v[1] - v[0], v[2] - v[1], v[0] - v[2]];
    let mut axes = vec![Vec3::X, Vec3::Y, Vec3::Z, edges[0].cross(edges[1])];
    for e in edges {
        for a in [Vec3::X, Vec3::Y, Vec3::Z] {
            axes.push(e.cross(a));
        }
    }
    axes.into_iter().all(|axis| {
        let d = v.map(|v| v.dot(axis));
        let r = 0.50001 * axis.abs().element_sum();
        d.into_iter().fold(f32::INFINITY, f32::min) <= r
            && d.into_iter().fold(f32::NEG_INFINITY, f32::max) >= -r
    })
}
fn closest_barycentric(p: [Vec3; 3], q: Vec3) -> Vec3 {
    let ab = p[1] - p[0];
    let ac = p[2] - p[0];
    let ap = q - p[0];
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0. && d2 <= 0. {
        return Vec3::X;
    }
    let bp = q - p[1];
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0. && d4 <= d3 {
        return Vec3::Y;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        let v = d1 / (d1 - d3);
        return Vec3::new(1. - v, v, 0.);
    }
    let cp = q - p[2];
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0. && d5 <= d6 {
        return Vec3::Z;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        let w = d2 / (d2 - d6);
        return Vec3::new(1. - w, 0., w);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return Vec3::new(0., 1. - w, w);
    }
    let denom = va + vb + vc;
    if denom.abs() < 1e-12 {
        return Vec3::X;
    }
    Vec3::new(va, vb, vc) / denom
}
fn fill(cells: &mut HashMap<[i32; 3], [u8; 3]>, n: u32) {
    // Traverse a padded grid so surfaces touching the bounds cannot seal the exterior.
    let n = n as i32;
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([[-1, -1, -1]]);
    seen.insert([-1, -1, -1]);
    while let Some(p) = queue.pop_front() {
        for d in DIRECTIONS {
            let q = std::array::from_fn(|i| p[i] + d[i]);
            if q.iter().all(|&x| x >= -1 && x <= n) && !cells.contains_key(&q) && seen.insert(q) {
                queue.push_back(q);
            }
        }
    }
    let color = cells.values().next().copied().unwrap_or([160, 180, 200]);
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let p = [x, y, z];
                if !seen.contains(&p) {
                    cells.entry(p).or_insert(color);
                }
            }
        }
    }
}
pub const DIRECTIONS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_intersection() {
        let t = [Vec3::ZERO, Vec3::X, Vec3::Y];
        assert!(triangle_box(t, Vec3::ZERO));
        assert!(!triangle_box(t, Vec3::splat(4.)));
    }
    #[test]
    fn barycentric_sample() {
        let b = closest_barycentric([Vec3::ZERO, Vec3::X, Vec3::Y], Vec3::new(0.25, 0.25, 0.));
        assert!((b - Vec3::new(0.5, 0.25, 0.25)).length() < 1e-6);
    }
    #[test]
    fn fills_only_closed_shell() {
        let mut cells = HashMap::new();
        for z in 0..5 {
            for y in 0..5 {
                for x in 0..5 {
                    if [x, y, z].iter().any(|&n| n == 0 || n == 4) {
                        cells.insert([x, y, z], [255, 0, 0]);
                    }
                }
            }
        }
        fill(&mut cells, 5);
        assert_eq!(cells.len(), 125);
        cells.remove(&[0, 2, 2]);
        cells.remove(&[1, 2, 2]);
        cells.remove(&[2, 2, 2]);
        fill(&mut cells, 5);
        assert!(!cells.contains_key(&[2, 2, 2]));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn textured_obj_voxelizes_and_fills() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/textured-cube.obj");
        let mesh = Mesh::load(&path).unwrap();
        assert_eq!(mesh.triangles.len(), 12);
        assert!(mesh.warnings.is_empty());
        assert!(mesh.materials.iter().any(|m| m.texture.is_some()));
        let shell = mesh.voxelize(16, false, true);
        let solid = mesh.voxelize(16, true, true);
        assert_eq!(solid.cells.len(), 16 * 16 * 16);
        assert_eq!(shell.cells.len(), 16 * 16 * 16 - 14 * 14 * 14);
        assert!(shell.cells.values().any(|&c| c == [183, 111, 193]));
        assert!(shell.cells.values().any(|&c| c == [244, 229, 206]));
        assert!(mesh.voxelize(8, false, true).cells.len() < shell.cells.len());
        let untextured = mesh.voxelize(8, false, false);
        assert!(untextured.cells.values().all(|&c| c == [255, 255, 255]));
    }
    #[test]
    fn missing_texture_is_reported_without_losing_geometry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("t.obj"),
            "mtllib t.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl Mat\nf 1 2 3\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("t.mtl"),
            "newmtl Mat\nKd 1 0 0\nmap_Kd missing.png\n",
        )
        .unwrap();
        let mesh = Mesh::load(&dir.path().join("t.obj")).unwrap();
        assert_eq!(mesh.warnings.len(), 1);
        assert!(!mesh.voxelize(8, false, true).cells.is_empty());
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;
    #[test]
    fn resolves_texture_relative_to_nested_mtl() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("materials")).unwrap();
        std::fs::write(
            dir.path().join("materials/checker.png"),
            include_bytes!("../assets/checker.png"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("materials/cube.mtl"),
            "newmtl Mat\nKd 1 1 1\nmap_Kd checker.png\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("cube.obj"),
            "mtllib materials/cube.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl Mat\nf 1 2 3\n",
        )
        .unwrap();
        let m = Mesh::load(&dir.path().join("cube.obj")).unwrap();
        assert!(m.warnings.is_empty());
        assert!(m.materials[0].texture.is_some());
    }
    #[test]
    #[ignore = "Requires a local Blender executable"]
    fn blender_import_preserves_packed_image() {
        let m =
            Mesh::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/textured-cube.blend"))
                .unwrap();
        assert_eq!(m.triangles.len(), 12);
        assert!(m.warnings.is_empty(), "{:?}", m.warnings);
        let v = m.voxelize(16, false, true);
        assert!(v.cells.values().any(|&c| c == [183, 111, 193]));
        assert!(v.cells.values().any(|&c| c == [244, 229, 206]));
    }
}
