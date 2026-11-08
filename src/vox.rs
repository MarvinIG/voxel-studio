use crate::model::Volume;
use std::{collections::HashMap, path::Path};

fn chunk(id: &[u8; 4], data: &[u8], children: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend((data.len() as u32).to_le_bytes());
    out.extend((children.len() as u32).to_le_bytes());
    out.extend(data);
    out.extend(children);
    out
}
pub fn save(volume: &Volume, path: &Path) -> Result<(), String> {
    if volume.cells.is_empty() {
        return Err("Ein leeres Modell kann nicht exportiert werden.".into());
    }
    if volume.size > 256 {
        return Err("VOX unterstützt maximal 256 Voxel pro Achse.".into());
    }
    // Exact colors for <=255 colors; otherwise a deterministic 6x6x6 RGB palette.
    let mut palette: Vec<_> = volume.cells.values().copied().collect();
    palette.sort();
    palette.dedup();
    let quantized = palette.len() > 255;
    if quantized {
        palette.clear();
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    palette.push([r * 51, g * 51, b * 51]);
                }
            }
        }
    }
    let indices: HashMap<_, _> = palette
        .iter()
        .enumerate()
        .map(|(i, c)| (*c, (i + 1) as u8))
        .collect();
    let mut xyz = Vec::new();
    xyz.extend((volume.cells.len() as u32).to_le_bytes());
    let mut cells: Vec<_> = volume.cells.iter().collect();
    cells.sort_by_key(|(p, _)| **p);
    for (p, c) in cells {
        if p.iter().any(|&x| x < 0 || x >= volume.size as i32) {
            return Err("Voxel außerhalb der Modellgrenzen.".into());
        }
        // Editor uses Y up; MagicaVoxel uses Z up. Rotate without changing handedness.
        let color = if quantized {
            c.map(|v| ((v as u16 + 25) / 51 * 51) as u8)
        } else {
            *c
        };
        xyz.extend([
            p[0] as u8,
            (volume.size as i32 - 1 - p[2]) as u8,
            p[1] as u8,
            indices[&color],
        ]);
    }
    let mut size = Vec::new();
    for _ in 0..3 {
        size.extend(volume.size.to_le_bytes());
    }
    let mut rgba = vec![0u8; 1024];
    for (i, c) in palette.iter().enumerate() {
        rgba[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
    }
    let children = [
        chunk(b"SIZE", &size, &[]),
        chunk(b"XYZI", &xyz, &[]),
        chunk(b"RGBA", &rgba, &[]),
    ]
    .concat();
    let data = [
        b"VOX ".to_vec(),
        150u32.to_le_bytes().to_vec(),
        chunk(b"MAIN", &[], &children),
    ]
    .concat();
    // Stage in the destination directory; an interrupted write leaves the old file intact.
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    use std::io::Write;
    staged
        .write_all(&data)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    staged.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn load(path: &Path) -> Result<Volume, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.get(..4) != Some(b"VOX ") {
        return Err("Ungültige VOX-Datei.".into());
    }
    fn u32_at(d: &[u8], i: usize) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            d.get(i..i + 4)
                .ok_or("Beschädigte VOX-Datei")?
                .try_into()
                .unwrap(),
        ))
    }
    let mut pos = 8;
    let mut size = None;
    let mut points = vec![];
    let mut palette = None;
    let mut models = 0;
    while pos + 12 <= data.len() {
        let id = &data[pos..pos + 4];
        let len = u32_at(&data, pos + 4)? as usize;
        let children = u32_at(&data, pos + 8)? as usize;
        pos += 12;
        if len > data.len() - pos || children > data.len() - pos - len {
            return Err("Beschädigte VOX-Chunklänge.".into());
        }
        match id {
            b"SIZE" => {
                if len < 12 {
                    return Err("Ungültiger SIZE-Chunk.".into());
                }
                let dims = [
                    u32_at(&data, pos)?,
                    u32_at(&data, pos + 4)?,
                    u32_at(&data, pos + 8)?,
                ];
                if dims.iter().any(|&n| n == 0 || n > 256) {
                    return Err("Ungültige VOX-Abmessungen.".into());
                }
                if size.is_none() {
                    size = Some(dims);
                }
            }
            b"XYZI" => {
                models += 1;
                if models > 1 {
                    return Err("Mehrteilige VOX-Szenen werden noch nicht unterstützt. Bitte als einzelnes Modell exportieren.".into());
                }
                if len < 4 {
                    return Err("Ungültiger XYZI-Chunk.".into());
                }
                let count = u32_at(&data, pos)? as usize;
                if count > (len - 4) / 4 {
                    return Err("Ungültige Voxelanzahl.".into());
                }
                points.extend(
                    data[pos + 4..pos + 4 + count * 4]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| [p[0], p[1], p[2], p[3]]),
                );
            }
            b"RGBA" => {
                if len != 1024 {
                    return Err("Ungültige Palette.".into());
                }
                palette = Some(
                    data[pos..pos + len]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| [c[0], c[1], c[2]])
                        .collect::<Vec<_>>(),
                );
            }
            _ => {}
        }
        pos += len; // Child chunks follow in the same byte stream.
    }
    let dims = size.ok_or("VOX enthält kein Modell.")?;
    if models == 0 {
        return Err("VOX enthält keine Voxeldaten.".into());
    }
    let n = *dims.iter().max().unwrap();
    let mut cells = HashMap::new();
    for [x, y, z, i] in points {
        if i == 0 || x as u32 >= dims[0] || y as u32 >= dims[1] || z as u32 >= dims[2] {
            return Err("Ungültiger Voxeleintrag.".into());
        }
        let color = if let Some(p) = &palette {
            p[i as usize - 1]
        } else {
            default_color(i)
        };
        cells.insert([x as i32, z as i32, n as i32 - 1 - y as i32], color);
    }
    Ok(Volume { size: n, cells })
}
fn default_color(index: u8) -> [u8; 3] {
    // MagicaVoxel's implicit palette: RGB cube, then red/green/blue/gray ramps.
    let i = index as usize - 1;
    if i < 215 {
        let levels = [255, 204, 153, 102, 51, 0];
        return [levels[i / 36], levels[(i / 6) % 6], levels[i % 6]];
    }
    let ramp = [238, 221, 187, 170, 136, 119, 85, 68, 34, 17];
    let j = i - 215;
    let v = ramp[(j % 10).min(9)];
    match j / 10 {
        0 => [v, 0, 0],
        1 => [0, v, 0],
        2 => [0, 0, v],
        _ => [v, v, v],
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_preserves_colors_and_axes() {
        let v = Volume {
            size: 32,
            cells: HashMap::from([([0, 1, 2], [13, 24, 35]), ([31, 30, 29], [255, 0, 12])]),
        };
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("test.vox");
        save(&v, &p).unwrap();
        assert_eq!(load(&p).unwrap(), v);
    }
    #[test]
    fn rejects_truncation() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("broken.vox");
        std::fs::write(&p, b"VOX \x96\0\0\0MAIN\xff\xff\xff\xff\0\0\0\0").unwrap();
        assert!(load(&p).is_err());
    }
    #[test]
    fn quantizes_many_colors() {
        let cells = (0..256)
            .map(|i| ([i % 16, i / 16, 0], [i as u8, (255 - i) as u8, 13]))
            .collect();
        let v = Volume { size: 16, cells };
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("many.vox");
        save(&v, &p).unwrap();
        let r = load(&p).unwrap();
        assert_eq!(r.cells.len(), 256);
        for (p, c) in v.cells {
            for (i, channel) in c.iter().enumerate() {
                assert!((*channel as i16 - r.cells[&p][i] as i16).abs() <= 25);
            }
        }
    }
}
