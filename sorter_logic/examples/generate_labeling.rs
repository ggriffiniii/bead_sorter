use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let default_path = "image_data".to_string();
    let data_dir_word = args.get(1).unwrap_or(&default_path);
    let data_dir = Path::new(data_dir_word);

    if !data_dir.exists() {
        eprintln!("Data directory not found: {:?}", data_dir);
        return Ok(());
    }

    let mut images = Vec::new();

    for entry in WalkDir::new(data_dir).min_depth(2).max_depth(2) {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "png") {
            let category = path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            let full_path = fs::canonicalize(path)?;
            let full_path_str = full_path.to_string_lossy().to_string();
            // Fix file:// scheme
            let url = format!("file://{}", full_path_str);

            images.push((category, filename, url));
        }
    }

    // Sort by category then filename
    images.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut file = File::create("labeling_tool.html")?;

    writeln!(
        file,
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Bead Empty Labeling Tool</title>
    <style>
        body {{ font-family: sans-serif; background: #222; color: #eee; padding: 20px; }}
        .header {{ position: sticky; top: 0; background: #333; padding: 10px; z-index: 100; border-bottom: 2px solid #555; display: flex; justify-content: space-between; align-items: center; }}
        .grid {{ display: flex; flex-wrap: wrap; gap: 10px; margin-top: 20px; }}
        .card {{ background: #444; border: 2px solid #555; border-radius: 8px; padding: 5px; width: 160px; text-align: center; cursor: pointer; position: relative; }}
        .card.selected {{ border-color: #f55; background: #633; }}
        .card img {{ width: 100%; height: 120px; object-fit: contain; background: #000; }}
        .card small {{ display: block; margin-top: 5px; font-size: 10px; overflow: hidden; text-overflow: ellipsis; }}
        .card .badge {{ position: absolute; top: 5px; right: 5px; background: #f55; color: white; padding: 2px 6px; border-radius: 4px; font-size: 10px; display: none; }}
        .card.selected .badge {{ display: block; }}
        button {{ padding: 10px 20px; font-size: 16px; background: #4caf50; color: white; border: none; cursor: pointer; border-radius: 4px; }}
        button:hover {{ background: #45a049; }}
        textarea {{ width: 100%; height: 100px; margin-top: 10px; background: #333; color: #fff; border: 1px solid #555; }}
    </style>
</head>
<body>
    <div class="header">
        <div>
            <h2>Empty Image Labeler</h2>
            <p>Click images to mark them as "Empty". then click "Generate List". Copy the result.</p>
        </div>
        <div>
            <span id="count">0 selected</span>
            <button onclick="exportList()">Generate List</button>
        </div>
    </div>

    <div id="output-area" style="display:none; margin: 20px 0; padding: 20px; background: #333;">
        <h3>Copy this list:</h3>
        <textarea id="output-text" readonly></textarea>
        <button onclick="document.getElementById('output-area').style.display='none'">Close</button>
    </div>

    <div class="grid">
"#
    )?;

    for (cat, name, url) in images {
        writeln!(
            file,
            r#"
        <div class="card" onclick="toggle(this)" data-name="{name}">
            <span class="badge">EMPTY</span>
            <img src="{url}" loading="lazy">
            <small>{cat}<br>{name}</small>
        </div>
"#,
            name = name,
            url = url,
            cat = cat
        )?;
    }

    writeln!(
        file,
        r#"
    </div>

    <script>
        let selected = new Set();

        function toggle(el) {{
            el.classList.toggle('selected');
            let name = el.getAttribute('data-name');
            if (selected.has(name)) {{
                selected.delete(name);
            }} else {{
                selected.add(name);
            }}
            document.getElementById('count').innerText = selected.size + ' selected';
        }}

        function exportList() {{
            let arr = Array.from(selected);
            let json = JSON.stringify(arr);
            let textArea = document.getElementById('output-text');
            textArea.value = json;
            document.getElementById('output-area').style.display = 'block';
            textArea.select();
        }}
    </script>
</body>
</html>
"#
    )?;

    println!("Generated labeling_tool.html");
    Ok(())
}
