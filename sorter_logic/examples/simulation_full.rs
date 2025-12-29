use image::RgbaImage;
use sorter_logic::{AnalysisConfig, Palette, PaletteMatch, Rgb, analyze_image_debug};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// --- HTML Report Template ---
const REPORT_TEMPLATE_HEAD: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <style>
        body { font-family: sans-serif; background: #222; color: #eee; }
        .bead-container { display: flex; flex-wrap: wrap; gap: 10px; padding: 10px; }
        .bead-card { 
            background: #333; border-radius: 8px; padding: 5px; width: 140px; text-align: center;
            position: relative;
        }
        .bead-img-container {
            position: relative;
            width: 128px;
            height: 128px;
            margin: 0 auto;
        }
        .bead-img { 
            width: 128px; height: 128px; object-fit: contain; border-radius: 4px; display: block;
        }
        .mask-overlay {
            position: absolute;
            top: 0;
            left: 0;
            width: 128px;
            height: 128px;
            pointer-events: none;
            opacity: 0.6;
            display: none; /* Hidden by default */
        }
        .meta { font-size: 10px; color: #aaa; margin-top: 4px; }
        .palette-section { margin-bottom: 20px; border: 1px solid #444; padding: 10px; }
        .palette-header { background: #444; padding: 5px; font-weight: bold; }
        .controls {
            position: sticky; top: 0; background: #222; padding: 10px; z-index: 100; border-bottom: 1px solid #555;
            display: flex; align-items: center; gap: 15px;
        }
        .stats { font-size: 14px; color: #ccc; }
        button { padding: 5px 10px; cursor: pointer; }
    </style>
    <script>
        function toggleMasks(checkbox) {
            const masks = document.querySelectorAll('.mask-overlay');
            masks.forEach(m => {
                m.style.display = checkbox.checked ? 'block' : 'none';
            });
        }
    </script>
</head>
<body>
    <div class="controls">
        <h1>Bead Sorter - Full Simulation Report</h1>
        <label><input type="checkbox" onchange="toggleMasks(this)"> Show Classification Masks (Green=Used, Blue=Center)</label>
    </div>
"#;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut data_dir_str = "image_data".to_string();

    // Simple arg parsing
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" | "-d" => {
                if i + 1 < args.len() {
                    data_dir_str = args[i + 1].clone();
                    i += 1;
                } else {
                    eprintln!("Error: --dir requires a path argument");
                    return;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let data_dir = Path::new(&data_dir_str);
    let report_path = "full_report.html";

    if !data_dir.exists() {
        println!("Error: Data directory not found: {:?}", data_dir);
        println!("Usage: cargo run --example simulation_full --release -- --dir <path_to_images>");
        return;
    }

    // --- Load Images ---
    println!("Loading images from {:?}...", data_dir);
    let mut images = Vec::new();
    for entry in WalkDir::new(data_dir).min_depth(1).max_depth(10) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "png") {
            let img = image::open(path).expect("failed to open image").into_rgb8();
            let (w, h) = img.dimensions();
            let mut data = Vec::with_capacity((w * h * 2) as usize);
            for p in img.pixels() {
                let r = (p[0] as u16 * 31) / 255;
                let g = (p[1] as u16 * 63) / 255;
                let b = (p[2] as u16 * 31) / 255;
                let rgb565 = (r << 11) | (g << 5) | b;
                data.extend_from_slice(&rgb565.to_be_bytes());
            }
            images.push((path.to_path_buf(), data, w as usize, h as usize));
        }
    }
    println!("Loaded {} beads.", images.len());

    // --- Simulation ---
    // User requested 30 palettes
    let mut palette: Palette<30> = Palette::new();

    // Storage for report
    // Map Palette Index -> List of (Path, Analysis, MaskBase64)
    let mut palette_bins: HashMap<usize, Vec<(PathBuf, sorter_logic::BeadAnalysis, String)>> =
        HashMap::new();
    let mut unclassified: Vec<(PathBuf, String, String)> = Vec::new(); // (Path, Reason, MaskBase64)

    // Clean output dir for report images
    let report_img_dir = Path::new("report_images");
    if !report_img_dir.exists() {
        fs::create_dir(report_img_dir).ok();
    }

    println!("Running Full Simulation (30 Palettes, Standard Config)...");

    let config = AnalysisConfig::default(); // Uses 60% filter
    let mut processed_c = 0;

    for (path, data, w, h) in &images {
        let filename = path.file_name().unwrap().to_str().unwrap();

        let mut mask_buffer = vec![0u8; w * h];

        // Analyze
        let analysis_opt = analyze_image_debug(data, *w, *h, Some(&mut mask_buffer), config);

        // Generate Mask Image (PNG Base64) for HTML
        let mask_base64 = generate_mask_base64(&mask_buffer, *w as u32, *h as u32);

        // Copy Original Image to report_images for easy viewing
        let dest_path = report_img_dir.join(filename);
        fs::copy(path, &dest_path).ok();
        let rel_path = format!("report_images/{}", filename);
        let path_buf = PathBuf::from(rel_path);

        if let Some(analysis) = analysis_opt {
            let match_result = palette.match_color(&analysis.average_color, analysis.variance, 200);
            match match_result {
                PaletteMatch::Match(idx) | PaletteMatch::NewEntry(idx) => {
                    palette.add_sample(idx, &analysis.average_color, analysis.variance);
                    palette_bins.entry(idx).or_insert_with(Vec::new).push((
                        path_buf,
                        analysis,
                        mask_base64,
                    ));
                }
                PaletteMatch::Full => {
                    unclassified.push((path_buf, "Palette Full".to_string(), mask_base64));
                }
            }
        } else {
            unclassified.push((path_buf, "Empty/Rejected".to_string(), mask_base64));
        }

        processed_c += 1;
        if processed_c % 50 == 0 {
            print!(".");
            std::io::stdout().flush().ok();
        }
    }
    println!("\nProcessed {}.", processed_c);

    // --- Generate HTML Report ---
    let mut file = File::create(report_path).unwrap();
    writeln!(file, "{}", REPORT_TEMPLATE_HEAD).unwrap();

    // Stats
    let total_palettes = palette.len();
    writeln!(file, "<div class='palette-section'><h2>Statistics</h2>").unwrap();
    writeln!(
        file,
        "<p>Total Images: {} <br> Active Palettes: {}/30 <br> Unclassified: {}</p></div>",
        images.len(),
        total_palettes,
        unclassified.len()
    )
    .unwrap();

    // Palettes
    // Sorted by index
    let mut indices: Vec<usize> = palette_bins.keys().cloned().collect();
    indices.sort();

    for idx in indices {
        let entry = palette.get_entry(idx).unwrap();
        let (avg_rgb, avg_var) = entry.avg();
        let beads = palette_bins.get(&idx).unwrap();

        // Hex Color for header
        let hex = format!("#{:02X}{:02X}{:02X}", avg_rgb.r, avg_rgb.g, avg_rgb.b);

        writeln!(file, "<div class='palette-section'>").unwrap();
        writeln!(file, "<div class='palette-header' style='border-left: 20px solid {};'>Palette {} - Count: {} - Center: RGB({},{},{}) Var:{}</div>", 
            hex, idx, beads.len(), avg_rgb.r, avg_rgb.g, avg_rgb.b, avg_var).unwrap();

        writeln!(file, "<div class='bead-container'>").unwrap();

        for (p, analysis, mask) in beads {
            let p_str = p.to_string_lossy();
            writeln!(file, "<div class='bead-card'>").unwrap();
            writeln!(file, "<div class='bead-img-container'>").unwrap();
            writeln!(file, "<img src='{}' class='bead-img'>", p_str).unwrap();
            writeln!(
                file,
                "<img src='data:image/png;base64,{}' class='mask-overlay'>",
                mask
            )
            .unwrap();
            writeln!(file, "</div>").unwrap();
            writeln!(
                file,
                "<div class='meta'>L:{:.0} a:{:.0} b:{:.0}<br>Var:{}</div>",
                // We re-calculate Lab for display or use RGB? Let's use RGB for now or just V.
                // Analysis struct has RGB.
                analysis.average_color.to_lab().0 as f32, // Rough hack
                analysis.average_color.to_lab().1 as f32,
                analysis.average_color.to_lab().2 as f32,
                analysis.variance
            )
            .unwrap();
            writeln!(file, "</div>").unwrap();
        }

        writeln!(file, "</div></div>").unwrap();
    }

    // Unclassified
    if !unclassified.is_empty() {
        writeln!(file, "<div class='palette-section'>").unwrap();
        writeln!(file, "<div class='palette-header' style='background:#500'>Unclassified / Rejected ({})</div>", unclassified.len()).unwrap();
        writeln!(file, "<div class='bead-container'>").unwrap();
        for (p, reason, mask) in &unclassified {
            let p_str = p.to_string_lossy();
            writeln!(file, "<div class='bead-card'>").unwrap();
            writeln!(file, "<div class='bead-img-container'>").unwrap();
            writeln!(file, "<img src='{}' class='bead-img'>", p_str).unwrap();
            writeln!(
                file,
                "<img src='data:image/png;base64,{}' class='mask-overlay'>",
                mask
            )
            .unwrap();
            writeln!(file, "</div>").unwrap();
            writeln!(file, "<div class='meta'>{}</div>", reason).unwrap();
            writeln!(file, "</div>").unwrap();
        }
        writeln!(file, "</div></div>").unwrap();
    }

    writeln!(file, "</body></html>").unwrap();
    println!("Report generated: {}", report_path);
}

// Helper to generate PNG Base64 from mask buffer
fn generate_mask_base64(mask: &[u8], width: u32, height: u32) -> String {
    let mut img = image::RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let val = if idx < mask.len() { mask[idx] } else { 0 };
            // Use same Colors as simulaton.rs
            // 1=Green(Selected), 2=Red, 3=Yellow, 4=Blue(Center)
            let color = match val {
                1 => image::Rgba([0, 255, 0, 100]),   // Green Translucent
                2 => image::Rgba([255, 0, 0, 100]),   // Red
                3 => image::Rgba([255, 255, 0, 100]), // Yellow
                4 => image::Rgba([0, 0, 255, 255]),   // Blue Solid
                _ => image::Rgba([0, 0, 0, 0]),
            };
            img.put_pixel(x, y, color);
        }
    }

    let mut buffer = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buffer);
    img.write_to(&mut cursor, image::ImageOutputFormat::Png)
        .unwrap();
    base64::encode(buffer)
}
