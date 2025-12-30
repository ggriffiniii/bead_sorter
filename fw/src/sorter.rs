use heapless::Vec;
use sorter_logic::{analyze_image, Palette, PaletteEntry, PaletteMatch};

pub struct BeadSorter {
    palette: Palette<128>,
    tubes: Vec<PaletteEntry, 30>,
    palette_to_tube: [u8; 128],
}

impl BeadSorter {
    pub fn new() -> Self {
        Self {
            palette: Palette::new(),
            tubes: Vec::new(),
            palette_to_tube: [0xFF; 128],
        }
    }

    pub fn get_tube_for_image(&mut self, buf_bytes: &[u8], w: usize, h: usize) -> Option<u8> {
        let analysis = analyze_image(buf_bytes, w, h)?;

        // Adaptive Learning
        let match_result = self
            .palette
            .match_color(&analysis.average_color, analysis.variance, 15);

        let p_idx = match match_result {
            PaletteMatch::Match(i) => Some(i),
            PaletteMatch::NewEntry(i) => Some(i),
            PaletteMatch::Full => None,
        }?;

        self.palette
            .add_sample(p_idx, &analysis.average_color, analysis.variance);

        let tid = if self.palette_to_tube[p_idx] != 0xFF {
            let t_idx = self.palette_to_tube[p_idx] as usize;
            defmt::info!("bead matched palette entry: {}, tube: {}", p_idx, t_idx);
            t_idx
        } else {
            if self.tubes.len() < 30 {
                defmt::info!(
                    "New Palette Entry: {} assigning to empty tube: {}",
                    p_idx,
                    self.tubes.len()
                );
                let entry = PaletteEntry::new(analysis.average_color, analysis.variance);
                self.tubes.push(entry).unwrap();
                self.tubes.len() - 1
            } else {
                let mut best_t = 0;
                let mut min_d = u32::MAX;
                for (t_i, t_entry) in self.tubes.iter().enumerate() {
                    let (t_avg, _) = t_entry.avg();
                    let d = analysis.average_color.dist_lab(&t_avg);
                    if d < min_d {
                        min_d = d;
                        best_t = t_i;
                    }
                }
                defmt::info!(
                    "New Palette Entry: {} no empty tubes; Next closest tube: {}",
                    p_idx,
                    best_t
                );
                best_t
            }
        };

        if p_idx < 128 {
            self.palette_to_tube[p_idx] = tid as u8;
        }

        if tid < self.tubes.len() {
            self.tubes[tid].add(analysis.average_color, analysis.variance);
        }

        Some(tid as u8)
    }
}
