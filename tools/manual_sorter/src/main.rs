use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use sorter_logic::{analyze_image_debug, AnalysisConfig, Palette, PaletteMatch};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tower_http::{cors::CorsLayer, services::ServeDir};
use walkdir::WalkDir;

#[derive(Clone, Serialize, Deserialize)]
struct Bead {
    id: usize,
    filename: String,
    path: String,
    assignment: String, // "p0".."p29", "unclassified", "empty"
    variance: u32,
    rgb: (u8, u8, u8),
}

struct AppState {
    beads: Vec<Bead>,
    input_dir: PathBuf,
    output_dir: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    // usage: manual_sorter --input <dir> --output <dir>
    let mut input_dir = PathBuf::from("image_data/assorted");
    let mut output_dir = PathBuf::from("sorted_output");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input" | "-i" => {
                if i + 1 < args.len() {
                    input_dir = PathBuf::from(&args[i + 1]);
                    i += 1;
                }
            }
            "--output" | "-o" => {
                if i + 1 < args.len() {
                    output_dir = PathBuf::from(&args[i + 1]);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    if !input_dir.exists() {
        eprintln!("Error: Input directory {:?} not found", input_dir);
        return;
    }

    println!("Initializing Bead Sorter...");
    println!("Input: {:?}", input_dir);
    println!("Output: {:?}", output_dir);

    // 1. First Pass Sort
    let beads = initial_sort(&input_dir);
    println!("Loaded {} beads.", beads.len());

    let state = Arc::new(Mutex::new(AppState {
        beads,
        input_dir: input_dir.clone(),
        output_dir,
    }));

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/state", get(get_state))
        .route("/api/move", post(move_bead))
        .route("/api/finalize", post(finalize_sort))
        .nest_service("/images", ServeDir::new(input_dir)) // Serve raw images
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Server running at http://localhost:3000");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// Logic to run sorter_logic pass
fn initial_sort(path: &PathBuf) -> Vec<Bead> {
    let mut beads = Vec::new();
    let mut palette: Palette<128> = Palette::new();
    let config = AnalysisConfig::default(); // 60% filter

    let mut id_counter = 0;

    for entry in WalkDir::new(path).min_depth(1).max_depth(10) {
        let entry = entry.unwrap();
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "png") {
            // Load Image
            let img = match image::open(p) {
                Ok(i) => i.into_rgb8(),
                Err(_) => continue,
            };
            let (w, h) = img.dimensions();

            // Convert to RGB565 for analyzer
            let mut data = Vec::with_capacity((w * h * 2) as usize);
            for px in img.pixels() {
                let r = (px[0] as u16 * 31) / 255;
                let g = (px[1] as u16 * 63) / 255;
                let b = (px[2] as u16 * 31) / 255;
                let rgb565 = (r << 11) | (g << 5) | b;
                data.extend_from_slice(&rgb565.to_be_bytes());
            }

            let mut assignment = "unclassified".to_string();
            let mut variance = 0;
            let mut rgb_disp = (0, 0, 0);

            if let Some(analysis) = analyze_image_debug(&data, w as usize, h as usize, None, config)
            {
                let match_result =
                    palette.match_color(&analysis.average_color, analysis.variance, 30);
                match match_result {
                    PaletteMatch::Match(idx) | PaletteMatch::NewEntry(idx) => {
                        palette.add_sample(idx, &analysis.average_color, analysis.variance);
                        assignment = format!("p{}", idx);
                    }
                    _ => {} // Full or otherwise -> unclassified
                }
                variance = analysis.variance;
                rgb_disp = (
                    analysis.average_color.r,
                    analysis.average_color.g,
                    analysis.average_color.b,
                );
            } else {
                assignment = "empty".to_string();
            }

            beads.push(Bead {
                id: id_counter,
                filename: p.file_name().unwrap().to_str().unwrap().to_string(),
                path: p.to_string_lossy().to_string(), // Absolute or relative needed? Relative needed for URL
                assignment,
                variance,
                rgb: rgb_disp,
            });
            id_counter += 1;
        }
    }
    beads
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../index.html"))
}

async fn get_state(State(state): State<Arc<Mutex<AppState>>>) -> Json<Vec<Bead>> {
    let state = state.lock().unwrap();
    Json(state.beads.clone())
}

#[derive(Deserialize)]
struct MoveReq {
    bead_id: usize,
    target_assignment: String,
}

async fn move_bead(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(payload): Json<MoveReq>,
) -> StatusCode {
    let mut state = state.lock().unwrap();
    if let Some(bead) = state.beads.iter_mut().find(|b| b.id == payload.bead_id) {
        bead.assignment = payload.target_assignment;
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn finalize_sort(State(state): State<Arc<Mutex<AppState>>>) -> String {
    let state = state.lock().unwrap();
    let out_base = &state.output_dir;

    if !out_base.exists() {
        std::fs::create_dir_all(out_base).ok();
    }

    let mut moved_count = 0;

    // Base dirs
    let unclassified_dir = out_base.join("unclassified");
    std::fs::create_dir_all(&unclassified_dir).ok();

    let empty_dir = out_base.join("empty");
    std::fs::create_dir_all(&empty_dir).ok();

    for bead in &state.beads {
        let target_dir = if bead.assignment.starts_with('p') {
            let idx_str = &bead.assignment[1..];
            out_base.join(format!("palette_{}", idx_str))
        } else if bead.assignment == "empty" {
            empty_dir.clone()
        } else {
            unclassified_dir.clone()
        };

        // Ensure dir exists (Dynamic Creation)
        if !target_dir.exists() {
            std::fs::create_dir_all(&target_dir).ok();
        }

        let target = target_dir.join(&bead.filename);

        // Copy instead of move for safety? User asked to "output groupings", usually implies organizing.
        // Move is destructive. Copy is safer. Let's Copy.
        if std::fs::copy(&bead.filename, &target).is_ok() {
            // Try relative path
            moved_count += 1;
        } else {
            // Try absolute via input_dir join
            let real_source = state.input_dir.join(&bead.filename);
            if std::fs::copy(&real_source, &target).is_ok() {
                moved_count += 1;
            }
        }
    }

    format!("Finalized! Copied {} beads to {:?}", moved_count, out_base)
}
