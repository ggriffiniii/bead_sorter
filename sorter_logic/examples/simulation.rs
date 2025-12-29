use image::RgbaImage;
use sorter_logic::{AnalysisConfig, Palette, PaletteMatch, Rgb, analyze_image_debug};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;

fn main() {
    let args: Vec<String> = env::args().collect();
    let default_path = "image_data".to_string();
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
    let mut palette: Palette<6> = Palette::new(); // User Constraint: 6 Palettes

    let mut total_processed = 0;
    let mut valid_dataset_size = 0;
    let mut palette_full_errors = 0;

    // Accuracy Tracking
    let mut correct_assignments = 0;
    let mut collision_errors = 0;
    let mut empty_count = 0;

    // User-provided list of Empty/Invalid images to ignore in scoring
    const IGNORE_LIST: &[&str] = &[
        "bead_1766961084091.png",
        "bead_1766961088634.png",
        "bead_1766961097684.png",
        "bead_1766961093142.png",
        "bead_1766962257269.png",
        "bead_1766962275377.png",
        "bead_1766962302499.png",
        "bead_1766962307043.png",
        "bead_1766962343225.png",
        "bead_1766962361303.png",
        "bead_1766962320606.png",
        "bead_1766962316061.png",
        "bead_1766962388426.png",
        "bead_1766962469862.png",
        "bead_1766962496989.png",
        "bead_1766962501531.png",
        "bead_1766962510551.png",
        "bead_1766961993202.png",
        "bead_1766961997711.png",
        "bead_1766962024864.png",
        "bead_1766962002253.png",
        "bead_1766962079141.png",
        "bead_1766962083653.png",
        "bead_1766962056528.png",
        "bead_1766962065580.png",
        "bead_1766962137932.png",
        "bead_1766962124372.png",
        "bead_1766962115321.png",
        "bead_1766962672155.png",
        "bead_1766962681209.png",
        "bead_1766962717385.png",
        "bead_1766962735457.png",
        "bead_1766961691824.png",
        "bead_1766961700875.png",
        "bead_1766961705418.png",
        "bead_1766961737080.png",
        "bead_1766961773256.png",
        "bead_1766961759694.png",
        "bead_1766961841107.png",
        "bead_1766961439546.png",
        "bead_1766961425952.png",
    ];

    println!(
        "Running Analysis with Empty Detection (Constrained to 6 Palettes, Lab Match, Thresh 200, VarWeight 0.10)..."
    );
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

    for (path, data, width, height) in images.iter() {
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let is_ignored = IGNORE_LIST.contains(&filename.as_str());
        let truth_category = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Use Actual Dimensions
        let (width, height) = (*width, *height);
        let mut mask = vec![0u8; width * height];

        if is_ignored {
            let abs_path = fs::canonicalize(path).unwrap();
            filtered_images.push(format!(
                 "<div class='bead-container filtered'><img src='file://{}' class='img-box'><br><small>{} (Ignored)</small></div>",
                 abs_path.to_string_lossy(),
                 truth_category
             ));
            empty_count += 1;
            continue;
        }

        let analysis = analyze_image_debug(
            data,
            width,
            height,
            Some(&mut mask),
            AnalysisConfig::default(),
        );

        total_processed += 1;

        if let Some(ana) = analysis {
            let match_result = palette.match_color(&ana.average_color, ana.variance, 200);

            let p_idx = match match_result {
                PaletteMatch::Match(i) => Some(i),
                PaletteMatch::NewEntry(i) => Some(i),
                PaletteMatch::Full => None,
            };

            // Generate Report HTML Snippet (Common for matched and unmatched)
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
                if is_ignored { "filtered" } else { "" },
                abs_bead_path.to_string_lossy(),
                abs_path.to_string_lossy(),
                ana.average_color.r,
                ana.average_color.g,
                ana.average_color.b,
                truth_category
            );

            if let Some(idx) = p_idx {
                // Update Learning (Only if not ignored)
                if !is_ignored {
                    palette.add_sample(idx, &ana.average_color, ana.variance);
                }

                // Calc Stats
                *palette_owners
                    .entry(idx)
                    .or_default()
                    .entry(truth_category.clone())
                    .or_default() += 1;

                assignments.push((filename.clone(), idx, truth_category.clone(), is_ignored));

                if !is_ignored {
                    valid_dataset_size += 1;
                }

                // Debug Info
                let (center, c_var) = palette.get_entry(idx).unwrap().avg();
                let dist_lab = ana.average_color.dist_lab(&center);
                let var_diff = (ana.variance as i64 - c_var as i64).abs();
                let dist_var = (var_diff / 10) as u32; // Use actual weight 1/10
                println!(
                    "DEBUG: {} ({}) -> P{} (L:{:?} V:{}) => D_Lab:{} D_Var:{} Tot:{}",
                    filename,
                    truth_category,
                    idx,
                    center.to_lab(),
                    c_var,
                    dist_lab,
                    dist_var,
                    dist_lab + dist_var
                );

                report_palettes.entry(idx).or_default().push(html_entry);
            } else {
                palette_full_errors += 1;
                if !is_ignored {
                    valid_dataset_size += 1;
                }
                println!("Full Palette Error: {}", filename);
                unclassified_images.push(html_entry);
            }
        } else {
            empty_count += 1;
            let abs_path = fs::canonicalize(path).unwrap();

            if !is_ignored {
                valid_dataset_size += 1;
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
    for (fname, pidx, truth, is_ignored) in assignments {
        if is_ignored {
            continue;
        } // Exclude from Accuracy

        let predicted_owner = p_owners.get(&pidx).unwrap();
        // Exception: Handle Palette merging? (e.g. White splits)
        // For now, Strict Majority Vote Ownership.

        if predicted_owner == &truth {
            correct_assignments += 1;
        } else {
            collision_errors += 1;
            let abs_path = fs::canonicalize(format!("image_data/{}/{}", truth, fname))
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
    println!("Assigned Correctly: {}", correct_assignments);
    println!("Assigned Incorrectly (Collisions): {}", collision_errors);
    println!("Unclassified (Palette Full): {}", palette_full_errors);

    if valid_dataset_size > 0 {
        let accuracy = (correct_assignments as f32 / valid_dataset_size as f32) * 100.0;
        println!("STRICT ACCURACY (Correct / Valid): {:.2}%", accuracy);
    }

    // Write Report
    writeln!(report_file, "<h2>Simulation Report</h2>").unwrap();
    if valid_dataset_size > 0 {
        writeln!(
            report_file,
            "<p><b>Strict Accuracy: {:.2}%</b> ({} / {})</p>",
            (correct_assignments as f32 / valid_dataset_size as f32) * 100.0,
            correct_assignments,
            valid_dataset_size
        )
        .unwrap();
    }
    writeln!(report_file, "<p>Ignored Entries: {}</p>", IGNORE_LIST.len()).unwrap();

    // Write Palettes
    writeln!(report_file, "<div style='display:flex; flex-wrap:wrap'>").unwrap();
    let mut sorted_indices: Vec<_> = report_palettes.keys().collect();
    sorted_indices.sort();

    for idx in sorted_indices {
        let entries = &report_palettes[idx];
        let owner = p_owners.get(idx).unwrap_or(&"unknown".to_string()).clone();
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
