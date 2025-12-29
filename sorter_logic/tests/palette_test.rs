use sorter_logic::{Palette, PaletteMatch, Rgb};

#[test]
fn test_palette_logic() {
    let mut palette: Palette<30> = Palette::new();

    let red = Rgb { r: 255, g: 0, b: 0 };
    let red_variant = Rgb {
        r: 250,
        g: 10,
        b: 10,
    }; // Dist approx (5^2 + 10^2 + 10^2) = 225
    let blue = Rgb { r: 0, g: 0, b: 255 };

    // 1. First bead -> New Entry 0
    match palette.match_color(&red, 0, 500) {
        PaletteMatch::NewEntry(idx) => assert_eq!(idx, 0),
        _ => panic!("Expected NewEntry(0)"),
    }

    // 2. Similar bead -> Match 0
    match palette.match_color(&red_variant, 0, 500) {
        PaletteMatch::Match(idx) => assert_eq!(idx, 0),
        _ => panic!("Expected Match(0)"),
    }

    // 3. Different bead -> New Entry 1
    match palette.match_color(&blue, 0, 500) {
        PaletteMatch::NewEntry(idx) => assert_eq!(idx, 1),
        _ => panic!("Expected NewEntry(1)"),
    }
}

#[test]
fn test_full_palette() {
    let mut palette: Palette<5> = Palette::new(); // Small palette for testing

    for i in 0..5 {
        let color = Rgb {
            r: i as u8 * 10,
            g: 0,
            b: 0,
        };
        match palette.match_color(&color, 0, 1) {
            // Very strict threshold
            PaletteMatch::NewEntry(idx) => assert_eq!(idx, i),
            _ => panic!("Expected NewEntry({})", i),
        }
    }

    // Now full (count = 5)
    let new_color = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    match palette.match_color(&new_color, 0, 100) {
        PaletteMatch::Full => (), // OK
        _ => panic!("Expected Full"),
    }
}
