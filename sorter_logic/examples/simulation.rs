use image::RgbaImage;
use sorter_logic::{AnalysisConfig, Palette, PaletteEntry, PaletteMatch, Rgb, analyze_image_debug};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;

fn main() {
    let args: Vec<String> = env::args().collect();
    let default_path = "image_data/full_sorted".to_string();
    let data_dir_word = args.get(1).unwrap_or(&default_path);
    let data_dir = Path::new(data_dir_word);

    if !data_dir.exists() {
        println!("Data directory not found: {:?}", data_dir);
        return;
    }

    // Load images with Dimensions
    println!("Loading images from {:?}...", data_dir);
    let mut images = Vec::new();
    for entry in WalkDir::new(data_dir).min_depth(2).max_depth(2) {
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

    // Shuffle images for simulation
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    images.shuffle(&mut thread_rng());

    println!("Loaded {} beads.", images.len());

    // --- Simulation Param ---
    let mut palette: Palette<128> = Palette::new(); // 128 Palettes allowed, will cluster to 30

    let mut total_processed = 0;
    let mut valid_dataset_size = 0;
    let mut palette_full_errors = 0;

    // Accuracy Tracking
    let mut correct_assignments = 0;
    let mut collision_errors = 0;
    let mut empty_count = 0;

    println!("Running Analysis (30 Palettes, Lab Match, Thresh 200, VarWeight 0.10)...");
    println!("Generating 'simulation_report.html'...");

    // ... (HTML gen is assumed done in previous step) ...
    // Note: I am skipping the manual HTML writing block in this replacement if it's already there?
    // Wait, replace_file_content targets specific lines.
    // I need to be careful not to overwrite the HTML I just wrote in lines 112-141.
    // The previous edit affected lines 112-141.
    // This edit targets lines 52 -> 108.
    // And lines 183 -> 231.
    // I should do this in 2 calls since they are far apart? Or 1 multi_replace?
    // Using multi_replace is safer.
    println!("Generating 'simulation_report.html'...");

    let mut report_file = File::create("simulation_report.html").unwrap();
    write!(report_file, "<html><head><style>
        body {{ font-family: sans-serif; background: #222; color: #eee; }} 
        .bead-container {{ display: inline-block; margin: 5px; text-align: center; width: 120px; vertical-align: top; }}
        .img-wrapper {{ position: relative; width: 100px; height: 100px; cursor: pointer; }}
        .img-box {{ width: 100px; height: 100px; object-fit: contain; border: 1px solid #555; background: #000; }}
        /* Mask is layered on top but hidden by default */
        .mask-overlay {{ position: absolute; top:0; left:0; width:100px; height:100px; object-fit: contain; opacity: 0; transition: opacity 0.2s; pointer-events: none; }}
        /* Show mask when parent wrapper has 'show-mask' class */
        .show-mask .mask-overlay {{ opacity: 0.8; }} 
        .color-swatch {{ width: 20px; height: 20px; display: inline-block; margin: 2px; border: 1px solid #fff; }}
        .palette {{ margin: 20px; padding: 10px; border: 1px solid #444; }}
        h3 {{ margin: 0 0 10px 0; }}
        .collision {{ border: 2px solid red; }}
        .filtered {{ opacity: 0.5; }}
        small {{ font-size: 10px; display: block; overflow: hidden; text-overflow: ellipsis; }}
        .legend {{ position: fixed; bottom: 20px; right: 20px; background: rgba(0,0,0,0.8); padding: 10px; border: 1px solid #777; z-index: 100; }}
        .controls {{ position: fixed; top: 20px; right: 20px; background: rgba(0,0,0,0.8); padding: 10px; border: 1px solid #777; z-index: 100; }}
    </style>
    <script>
        function toggleMask(el) {{
            el.classList.toggle('show-mask');
        }}
        function toggleAll(cb) {{
            const wrappers = document.querySelectorAll('.img-wrapper');
            wrappers.forEach(el => {{
                if (cb.checked) {{
                    el.classList.add('show-mask');
                }} else {{
                    el.classList.remove('show-mask');
                }}
            }});
        }}
    </script>
    </head><body>
    <div class='controls'>
        <label><input type='checkbox' onchange='toggleAll(this)'> Toggle All Masks</label>
    </div>
    <div class='legend'>
        <b>Legend:</b><br>
        <span style='color:#0f0'>Green Ring</span>: Search Area<br>
        <span style='color:#00f'>Blue Dot</span>: Detected Center<br>
        <span style='color:#f00'>Red Pixels</span>: Edges (Ignored)<br>
        <i>Click image to show/hide mask</i>
    </div>").unwrap();

    let mut report_palettes: HashMap<usize, Vec<String>> = HashMap::new();
    let mut collisions = Vec::new();
    let mut filtered_images = Vec::new();
    let mut unclassified_images = Vec::new(); // New List

    // Grouping for Accuracy
    let mut palette_owners: HashMap<usize, HashMap<String, u32>> = HashMap::new();
    let mut assignments: Vec<(String, usize, String, bool)> = Vec::new(); // (File, PalIdx, Truth, Ignored)

    // Ensure assets dir exists
    fs::create_dir_all("simulation_report_assets").ok();

    // Tube ID -> Tube Stats (Weighted Average of everything dropped in it)
    let mut tubes: Vec<PaletteEntry> = Vec::new(); // Max 30
    // Palette ID -> Tube ID
    let mut palette_to_tube: HashMap<usize, usize> = HashMap::new();
    let max_phys_tubes = 30;

    for (path, data, width, height) in images.iter() {
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let truth_category = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let is_empty_image = truth_category == "empty";

        // Use Actual Dimensions
        let (width, height) = (*width, *height);
        let mut mask = vec![0u8; width * height];

        let analysis = analyze_image_debug(
            data,
            width,
            height,
            Some(&mut mask),
            AnalysisConfig::default(),
        );

        total_processed += 1;

        if let Some(ana) = analysis {
            // Adaptive Threshold: 15
            let match_result = palette.match_color(&ana.average_color, ana.variance, 15);

            let p_idx = match match_result {
                PaletteMatch::Match(i) => Some(i),
                PaletteMatch::NewEntry(i) => Some(i),
                PaletteMatch::Full => None,
            };

            // Generate Report HTML Snippet
            let mut mask_img = RgbaImage::new(width as u32, height as u32);
            for y in 0..height {
                for x in 0..width {
                    let val = mask[y * width + x];
                    let pixel = match val {
                        1 => image::Rgba([0, 255, 0, 255]), // Green Ring
                        3 => image::Rgba([255, 0, 0, 255]), // Red Edge
                        4 => image::Rgba([0, 0, 255, 255]), // Blue Center
                        _ => image::Rgba([0, 0, 0, 0]),     // Transparent
                    };
                    mask_img.put_pixel(x as u32, y as u32, pixel);
                }
            }
            let mask_path = format!("simulation_report_assets/{}_mask.png", filename);
            mask_img.save(&mask_path).unwrap();
            let abs_path =
                fs::canonicalize(&mask_path).unwrap_or(Path::new(&mask_path).to_path_buf());
            let abs_bead_path = fs::canonicalize(path).unwrap();

            let html_entry = format!(
                "<div class='bead-container {}'><div class='img-wrapper' onclick='toggleMask(this)'><img src='file://{}' class='img-box'><img src='file://{}' class='mask-overlay'></div><div class='color-swatch' style='background-color: rgb({},{},{})'></div><small>{}</small></div>",
                if is_empty_image { "filtered" } else { "" },
                abs_bead_path.to_string_lossy(),
                abs_path.to_string_lossy(),
                ana.average_color.r,
                ana.average_color.g,
                ana.average_color.b,
                truth_category
            );

            if let Some(idx) = p_idx {
                // Update Palette Learning (Only if not ignored)
                if !is_empty_image {
                    palette.add_sample(idx, &ana.average_color, ana.variance);
                }

                // --- ONLINE TUBE ASSIGNMENT ---
                // Determine which Tube this Palette belongs to
                let tube_id = if let Some(tid) = palette_to_tube.get(&idx) {
                    *tid
                } else {
                    // New Palette! Assign to a Tube.
                    let new_tid = if tubes.len() < max_phys_tubes {
                        // Create New Tube
                        tubes.push(PaletteEntry::new(ana.average_color, ana.variance));
                        tubes.len() - 1
                    } else {
                        // Find Closest Tube
                        let mut best_t = 0;
                        let mut min_d = u32::MAX;
                        for (t_i, t_entry) in tubes.iter().enumerate() {
                            let (t_avg, _) = t_entry.avg();
                            let d = ana.average_color.dist_lab(&t_avg);
                            if d < min_d {
                                min_d = d;
                                best_t = t_i;
                            }
                        }
                        best_t
                    };

                    palette_to_tube.insert(idx, new_tid);
                    // println!("DEBUG: Palette {} mapped to Tube {} (New? {})", idx, new_tid, tubes.len() <= max_phys_tubes);
                    new_tid
                };

                // Update Tube Stats (Weighted Average)
                if !is_empty_image {
                    // Note: We might want to use a rolling average or just sum?
                    // PaletteEntry supports accumulation.
                    // But we need to be careful not to double count if we re-use PaletteEntry.
                    // Since `tubes` is a separate Vec, we can just `add`.
                    if tube_id < tubes.len() {
                        tubes[tube_id].add(ana.average_color, ana.variance);
                    }
                }
                // ------------------------------

                *palette_owners
                    .entry(idx)
                    .or_default()
                    .entry(truth_category.clone())
                    .or_default() += 1;

                assignments.push((
                    filename.clone(),
                    idx,
                    truth_category.clone(),
                    is_empty_image,
                ));

                if !is_empty_image {
                    valid_dataset_size += 1;
                }

                // Debug Info
                let (center, _) = palette.get_entry(idx).unwrap().avg();
                let dist_lab = ana.average_color.dist_lab(&center);
                println!(
                    "DEBUG: {} ({}) -> P{} -> Tube {} (L:{:?}) => D:{}",
                    filename,
                    truth_category,
                    idx,
                    tube_id,
                    center.to_lab(),
                    dist_lab
                );

                report_palettes.entry(idx).or_default().push(html_entry);
            } else {
                palette_full_errors += 1;
                valid_dataset_size += 1;
                println!("Full Palette Error: {}", filename);
                unclassified_images.push(html_entry);
            }
        } else {
            empty_count += 1;
            let abs_path = fs::canonicalize(path).unwrap();

            if !is_empty_image {
                valid_dataset_size += 1;
                collision_errors += 1; // False Negative
            }

            filtered_images.push(format!(
                 "<div class='bead-container filtered'><img src='file://{}' class='img-box'><br><small>{}</small></div>",
                 abs_path.to_string_lossy(), truth_category
             ));
        }
    }

    // --- Scoring ---
    // Calculate Owner for each palette
    let mut p_owners: HashMap<usize, String> = HashMap::new();
    for (pidx, counts) in &palette_owners {
        let mut max_c = 0;
        let mut owner = "unknown".to_string();
        for (cat, c) in counts {
            if *c > max_c {
                max_c = *c;
                owner = cat.clone();
            }
        }
        p_owners.insert(*pidx, owner);
    }

    // Evaluate Assignments
    for (fname, pidx, truth, is_empty_img) in assignments {
        if is_empty_img {
            correct_assignments += 1;
            continue;
        }

        let predicted_owner = p_owners.get(&pidx).unwrap();

        if predicted_owner == &truth {
            correct_assignments += 1;
        } else {
            collision_errors += 1;
            let abs_path = fs::canonicalize(data_dir.join(format!("{}/{}", truth, fname)))
                .unwrap_or_else(|_| Path::new("?").to_path_buf());
            let (center, _) = palette
                .get(pidx)
                .map(|rgb| (rgb, 0))
                .unwrap_or((Rgb { r: 0, g: 0, b: 0 }, 0));
            collisions.push(format!(
                 "<div class='bead-container collision'><img src='file://{}' class='img-box'><div class='color-swatch' style='background-color: rgb({},{},{})'></div><small>{}</small></div>",
                 abs_path.to_string_lossy(), center.r, center.g, center.b, truth
             ));
        }
    }

    println!("Total Processed: {}", total_processed);
    println!("Empty / Rejected: {}", empty_count);
    println!("Valid Dataset Size: {}", valid_dataset_size);
    println!("Assigned Correctly (Inc. Empty): {}", correct_assignments);
    println!("Assigned Incorrectly (Collisions): {}", collision_errors);
    println!("Unclassified (Palette Full): {}", palette_full_errors);

    if valid_dataset_size > 0 {
        let accuracy = (correct_assignments as f32 / valid_dataset_size as f32) * 100.0;
        println!("ACCURACY: {:.2}%", accuracy);
    }

    // Write Report
    writeln!(
        report_file,
        "<h2>Simulation Report (Online Clustering)</h2>"
    )
    .unwrap();
    if valid_dataset_size > 0 {
        writeln!(
            report_file,
            "<p><b>Accuracy: {:.2}%</b> ({} / {})</p>",
            (correct_assignments as f32 / valid_dataset_size as f32) * 100.0,
            correct_assignments,
            valid_dataset_size
        )
        .unwrap();
    }
    writeln!(
        report_file,
        "<p>Palettes Created: {} | Tubes Used: {}</p>",
        report_palettes.len(),
        tubes.len()
    )
    .unwrap();

    // Group Palettes by Tube for Report
    let mut tube_groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (pidx, tid) in &palette_to_tube {
        tube_groups.entry(*tid).or_default().push(*pidx);
    }

    let mut sorted_tubes: Vec<usize> = tube_groups.keys().cloned().collect();
    sorted_tubes.sort();

    for tube_idx in sorted_tubes {
        let palette_indices = &tube_groups[&tube_idx];

        let (t_avg, _) = tubes[tube_idx].avg();
        writeln!(
            report_file,
            "<div style='border: 2px solid #555; padding: 10px; margin: 10px; background: #333;'>"
        )
        .unwrap();
        writeln!(report_file, "<h2 style='margin-top:0'>Tube {} ({} Palettes) - Avg: <span class='color-swatch' style='background-color:rgb({},{},{})'></span></h2><div style='display:flex; flex-wrap:wrap'>", 
            tube_idx + 1, palette_indices.len(), t_avg.r, t_avg.g, t_avg.b).unwrap();

        let mut sorted_p_indices = palette_indices.clone();
        sorted_p_indices.sort();

        for idx in sorted_p_indices {
            let entries = &report_palettes[&idx];
            let owner = p_owners.get(&idx).unwrap_or(&"unknown".to_string()).clone();
            writeln!(
                report_file,
                "<div class='palette' style='border-color: #777'><h3>Palette {} ({})</h3>",
                idx, owner
            )
            .unwrap();
            for html in entries {
                writeln!(report_file, "{}", html).unwrap();
            }
            writeln!(report_file, "</div>").unwrap();
        }
        writeln!(report_file, "</div></div>").unwrap();
    }

    if !unclassified_images.is_empty() {
        writeln!(
            report_file,
            "<div class='palette' style='border-color: #f00'><h3>Unclassified / Rejected ({})</h3>",
            unclassified_images.len()
        )
        .unwrap();
        for html in unclassified_images {
            writeln!(report_file, "{}", html).unwrap();
        }
        writeln!(report_file, "</div>").unwrap();
    }

    // Write Collisions
    writeln!(report_file, "</div><div class='palette' style='border-color: red'><h3>Collisions / Errors (Valid Dataset)</h3>").unwrap();
    for html in collisions {
        writeln!(report_file, "{}", html).unwrap();
    }
    writeln!(report_file, "</div>").unwrap();

    // Write Filtered
    writeln!(
        report_file,
        "<div class='palette' style='border-color: #777'><h3>Filtered / Empty ({})</h3>",
        filtered_images.len()
    )
    .unwrap();
    for html in filtered_images {
        writeln!(report_file, "{}", html).unwrap();
    }
    writeln!(report_file, "</div></body></html>").unwrap();

    println!("Report generated.");
}
