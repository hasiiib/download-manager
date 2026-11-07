// Generate the preview icon deterministically at build time.
// This avoids fragile binary files in GitHub Contents API commits.
fn main() {
    use std::path::Path;
    let dir = Path::new("icons");
    std::fs::create_dir_all(dir).expect("create icon directory");
    let file = std::fs::File::create(dir.join("icon.png")).expect("create preview icon");
    let mut encoder = png::Encoder::new(file, 64, 64);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write PNG header");
    let mut pixels = vec![0u8; 64 * 64 * 4];
    for y in 0..64usize {
        for x in 0..64usize {
            let index = (y * 64 + x) * 4;
            let in_tile = (5..59).contains(&x) && (5..59).contains(&y);
            let arrow = ((29..35).contains(&x) && (14..42).contains(&y))
                || ((22..42).contains(&y) && x.abs_diff(32) + y.abs_diff(42) < 21)
                || ((21..43).contains(&x) && (47..52).contains(&y));
            let color: [u8; 4] = if arrow {
                [255, 255, 255, 255]
            } else if in_tile {
                [113, 89, 236, 255]
            } else {
                [0, 0, 0, 0]
            };
            pixels[index..index + 4].copy_from_slice(&color);
        }
    }
    writer.write_image_data(&pixels).expect("write PNG pixels");
    drop(writer);
    tauri_build::build();
}
