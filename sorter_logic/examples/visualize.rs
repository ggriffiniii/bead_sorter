use image::io::Reader as ImageReader;
use sorter_logic::analyze_image;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    let default_path = "../tools/image_saver/images".to_string();
    let img_dir_str = args.get(1).unwrap_or(&default_path);
    let img_dir = Path::new(img_dir_str);

    if !img_dir.exists() {
        println!("Image directory not found: {:?}", img_dir);
        println!("Usage: cargo run --example visualize -- [path/to/images]");
        return;
    }

    let mut entries: Vec<_> = fs::read_dir(img_dir)
        .unwrap()
        .filter_map(|res| res.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "png"))
        .collect();

    entries.sort();

    println!("Scanning {} images in {:?}...", entries.len(), img_dir);

    // Dynamic Palette
    let mut palette: sorter_logic::Palette<30> = sorter_logic::Palette::new();

    // Visualize all images
    for path in entries.iter() {
        println!("\n--- Image: {:?} ---", path.file_name().unwrap());
        let img = match ImageReader::open(path) {
            Ok(reader) => match reader.decode() {
                Ok(img) => img.to_rgb8(),
                Err(e) => {
                    println!("Failed to decode image: {}", e);
                    continue;
                }
            },
            Err(e) => {
                println!("Failed to open file: {}", e);
                continue;
            }
        };
        let (w, h) = img.dimensions();

        // ASCII Art - Saturation
        println!("Saturation Map:");
        for y in 0..h {
            for x in 0..w {
                let p = img.get_pixel(x, y);
                let r = p[0] as i16;
                let g = p[1] as i16;
                let b = p[2] as i16;
                let max = r.max(g).max(b);
                let min = r.min(g).min(b);
                let sat = max - min;

                if sat > 40 {
                    print!("S");
                } else {
                    print!(".");
                }
            }
            println!();
        }

        // Print Corners
        println!("Top-Left: {:?}", img.get_pixel(0, 0));
        println!("Top-Center: {:?}", img.get_pixel(w / 2, 0));
        println!("Top-Right: {:?}", img.get_pixel(w - 1, 0));
        println!("Bot-Left: {:?}", img.get_pixel(0, h - 1));
        println!("Bot-Right: {:?}", img.get_pixel(w - 1, h - 1));

        // Run Analysis
        let mut raw_data = Vec::new();
        for p in img.pixels() {
            // RGB888 -> RGB565 (Big Endian)
            let r = ((p[0] as u16) * 31) / 255;
            let g = ((p[1] as u16) * 63) / 255;
            let b = ((p[2] as u16) * 31) / 255;
            let rgb565 = (r << 11) | (g << 5) | b;
            raw_data.push((rgb565 >> 8) as u8);
            raw_data.push((rgb565 & 0xFF) as u8);
        }

        let analysis = analyze_image(&raw_data, w as usize, h as usize);
        println!("Analysis: {:?}", analysis);

        if let Some(ana) = analysis {
            match palette.match_color(&ana.average_color, ana.variance, 2000) {
                // High threshold for demo
                sorter_logic::PaletteMatch::Match(idx) => println!("Matched Palette #{}", idx),
                sorter_logic::PaletteMatch::NewEntry(idx) => println!("Added to Palette #{}", idx),
                sorter_logic::PaletteMatch::Full => println!("Palette Full!"),
            }
        }
    }
}
