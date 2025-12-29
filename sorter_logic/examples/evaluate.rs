use image::io::Reader as ImageReader;
use sorter_logic::Rgb;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Default, Debug)]
struct CatStats {
    count: u32,
    sum_r: u64,
    sum_g: u64,
    sum_b: u64,
    sum_sat: u64,
    sum_lum: u64,
}

impl CatStats {
    fn add(&mut self, rgb: &Rgb) {
        let max = rgb.r.max(rgb.g).max(rgb.b);
        let min = rgb.r.min(rgb.g).min(rgb.b);
        let sat = max - min;
        let lum = (rgb.r as u32 + rgb.g as u32 + rgb.b as u32) / 3;

        self.count += 1;
        self.sum_r += rgb.r as u64;
        self.sum_g += rgb.g as u64;
        self.sum_b += rgb.b as u64;
        self.sum_sat += sat as u64;
        self.sum_lum += lum as u64;
    }

    fn print(&self, name: &str, algo: &str) {
        if self.count == 0 {
            return;
        }
        println!(
            "{},{},{},{},{},{},{},{}",
            name,
            algo,
            self.sum_r / self.count as u64,
            self.sum_g / self.count as u64,
            self.sum_b / self.count as u64,
            self.sum_sat / self.count as u64,
            self.sum_lum / self.count as u64,
            self.count
        );
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let default_path = "image_data".to_string();
    let data_dir_str = args.get(1).unwrap_or(&default_path);
    let data_dir = Path::new(data_dir_str);

    if !data_dir.exists() {
        println!("Data directory not found: {:?}", data_dir);
        return;
    }

    println!("Category,Algo,R,G,B,Sat,Lum,Count");

    let mut stats: HashMap<String, HashMap<String, CatStats>> = HashMap::new();

    for entry in WalkDir::new(data_dir).min_depth(2).max_depth(2) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "png") {
            let category = path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            let img = match ImageReader::open(path) {
                Ok(r) => match r.decode() {
                    Ok(i) => i.to_rgb8(),
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            let (w, h) = img.dimensions();
            let pixels: Vec<Rgb> = img
                .pixels()
                .map(|p| Rgb {
                    r: p[0],
                    g: p[1],
                    b: p[2],
                })
                .collect();

            // Init maps
            stats
                .entry(category.clone())
                .or_default()
                .entry("Global".to_string())
                .or_default();
            stats
                .entry(category.clone())
                .or_default()
                .entry("Center".to_string())
                .or_default();
            stats
                .entry(category.clone())
                .or_default()
                .entry("Brightest20".to_string())
                .or_default();

            // Algo 1: Global Average
            let avg_global = average_rgb(&pixels);
            stats
                .get_mut(&category)
                .unwrap()
                .get_mut("Global")
                .unwrap()
                .add(&avg_global);

            // Algo 2: Center 50%
            let center_pixels: Vec<Rgb> = img
                .enumerate_pixels()
                .filter(|(x, y, _)| *x > w / 4 && *x < 3 * w / 4 && *y > h / 4 && *y < 3 * h / 4)
                .map(|(_, _, p)| Rgb {
                    r: p[0],
                    g: p[1],
                    b: p[2],
                })
                .collect();
            let avg_center = average_rgb(&center_pixels);
            stats
                .get_mut(&category)
                .unwrap()
                .get_mut("Center")
                .unwrap()
                .add(&avg_center);

            // Algo 3: Brightest 20%
            let mut sorted_by_luma = pixels.clone();
            sorted_by_luma.sort_by_key(|p| (p.r as u32 + p.g as u32 + p.b as u32));
            let top_20_count = pixels.len() / 5;
            let brightest = &sorted_by_luma[pixels.len().saturating_sub(top_20_count)..];
            let avg_bright = average_rgb(brightest);
            stats
                .get_mut(&category)
                .unwrap()
                .get_mut("Brightest20")
                .unwrap()
                .add(&avg_bright);
        }
    }

    // Print Results
    let mut sorted_cats: Vec<_> = stats.keys().collect();
    sorted_cats.sort();

    for cat in sorted_cats {
        let algos = stats.get(cat).unwrap();
        if let Some(s) = algos.get("Global") {
            s.print(cat, "Global");
        }
        if let Some(s) = algos.get("Center") {
            s.print(cat, "Center");
        }
        if let Some(s) = algos.get("Brightest20") {
            s.print(cat, "Brightest20");
        }
    }
}

fn average_rgb(pixels: &[Rgb]) -> Rgb {
    if pixels.is_empty() {
        return Rgb { r: 0, g: 0, b: 0 };
    }
    let mut sum_r = 0u32;
    let mut sum_g = 0u32;
    let mut sum_b = 0u32;
    for p in pixels {
        sum_r += p.r as u32;
        sum_g += p.g as u32;
        sum_b += p.b as u32;
    }
    let count = pixels.len() as u32;
    Rgb {
        r: (sum_r / count) as u8,
        g: (sum_g / count) as u8,
        b: (sum_b / count) as u8,
    }
}
