#![no_std]
use micromath::F32Ext;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaletteMatch {
    Match(usize),    // Index of matched entry
    NewEntry(usize), // Index of newly added entry
    Full,            // Palette is full, no match found
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaletteEntry {
    pub sum_r: u32,
    pub sum_g: u32,
    pub sum_b: u32,
    pub sum_var: u64,
    pub count: u32,
}

impl PaletteEntry {
    pub fn new(rgb: Rgb, var: u32) -> Self {
        Self {
            sum_r: rgb.r as u32,
            sum_g: rgb.g as u32,
            sum_b: rgb.b as u32,
            sum_var: var as u64,
            count: 1,
        }
    }

    pub fn add(&mut self, rgb: Rgb, var: u32) {
        self.sum_r += rgb.r as u32;
        self.sum_g += rgb.g as u32;
        self.sum_b += rgb.b as u32;
        self.sum_var += var as u64;
        self.count += 1;
    }

    pub fn avg(&self) -> (Rgb, u32) {
        if self.count == 0 {
            (Rgb { r: 0, g: 0, b: 0 }, 0)
        } else {
            (
                Rgb {
                    r: (self.sum_r / self.count) as u8,
                    g: (self.sum_g / self.count) as u8,
                    b: (self.sum_b / self.count) as u8,
                },
                (self.sum_var / self.count as u64) as u32,
            )
        }
    }
}

pub struct Palette<const N: usize> {
    colors: [Option<PaletteEntry>; N],
    count: usize,
}

impl<const N: usize> Palette<N> {
    pub const fn new() -> Self {
        Self {
            colors: [None; N],
            count: 0,
        }
    }

    /// Match a bead color & variance against the palette.
    /// Recommended Threshold: 100.
    pub fn match_color(&mut self, rgb: &Rgb, variance: u32, threshold: u32) -> PaletteMatch {
        for (i, entry) in self.colors.iter().enumerate() {
            if let Some(entry) = entry {
                let (center_rgb, center_var) = entry.avg();
                let dist_lab = rgb.dist_lab(&center_rgb);

                // Variance Diff Weight: 0.10 (1/10) - Tuned for 8-palette split
                let var_diff = (variance as i64 - center_var as i64).abs();
                let dist_var = (var_diff / 10) as u32;

                let total_dist = dist_lab + dist_var;

                if total_dist < threshold {
                    return PaletteMatch::Match(i);
                }
            } else {
                break;
            }
        }

        if self.count < N {
            let idx = self.count;
            self.colors[idx] = Some(PaletteEntry::new(*rgb, variance));
            self.count += 1;
            PaletteMatch::NewEntry(idx)
        } else {
            PaletteMatch::Full
        }
    }

    pub fn add_sample(&mut self, index: usize, rgb: &Rgb, variance: u32) {
        if index < N {
            if let Some(entry) = &mut self.colors[index] {
                entry.add(*rgb, variance);
            }
        }
    }

    pub fn get(&self, index: usize) -> Option<Rgb> {
        if index < N {
            self.colors[index].map(|e| e.avg().0)
        } else {
            None
        }
    }

    pub fn get_entry(&self, index: usize) -> Option<PaletteEntry> {
        if index < N { self.colors[index] } else { None }
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

impl Rgb {
    pub fn from_rgb565(p: u16) -> Self {
        let r = ((p >> 11) & 0x1F) as u8;
        let g = ((p >> 5) & 0x3F) as u8;
        let b = (p & 0x1F) as u8;

        // Scale to 8-bit
        let r8 = ((r as u16 * 255) / 31) as u8;
        let g8 = ((g as u16 * 255) / 63) as u8;
        let b8 = ((b as u16 * 255) / 31) as u8;

        Self {
            r: r8,
            g: g8,
            b: b8,
        }
    }

    pub fn dist(&self, other: &Rgb) -> u32 {
        // Use squared Euclidean
        let rd = (self.r as i32 - other.r as i32).pow(2);
        let gd = (self.g as i32 - other.g as i32).pow(2);
        let bd = (self.b as i32 - other.b as i32).pow(2);
        (rd + gd + bd) as u32
    }

    pub fn to_lab(&self) -> (i32, i32, i32) {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;

        let r = if r > 0.04045 {
            ((r + 0.055) / 1.055).powf(2.4)
        } else {
            r / 12.92
        };
        let g = if g > 0.04045 {
            ((g + 0.055) / 1.055).powf(2.4)
        } else {
            g / 12.92
        };
        let b = if b > 0.04045 {
            ((b + 0.055) / 1.055).powf(2.4)
        } else {
            b / 12.92
        };

        let x = (r * 0.4124 + g * 0.3576 + b * 0.1805) * 100.0;
        let y = (r * 0.2126 + g * 0.7152 + b * 0.0722) * 100.0;
        let z = (r * 0.0193 + g * 0.1192 + b * 0.9505) * 100.0;

        let x = x / 95.047;
        let y = y / 100.000;
        let z = z / 108.883;

        let x = if x > 0.008856 {
            x.powf(1.0 / 3.0)
        } else {
            (7.787 * x) + (16.0 / 116.0)
        };
        let y = if y > 0.008856 {
            y.powf(1.0 / 3.0)
        } else {
            (7.787 * y) + (16.0 / 116.0)
        };
        let z = if z > 0.008856 {
            z.powf(1.0 / 3.0)
        } else {
            (7.787 * z) + (16.0 / 116.0)
        };

        let l = (116.0 * y) - 16.0;
        let a = 500.0 * (x - y);
        let b = 200.0 * (y - z);

        (l as i32, a as i32, b as i32)
    }

    pub fn dist_lab(&self, other: &Rgb) -> u32 {
        let (l1, a1, b1) = self.to_lab();
        let (l2, a2, b2) = other.to_lab();
        ((l1 - l2).pow(2) + (a1 - a2).pow(2) + (b1 - b2).pow(2)) as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalysisConfig {
    pub edge_threshold: i32,
    pub min_dimension: usize,
    pub aspect_ratio_min: f32,
    pub aspect_ratio_max: f32,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            edge_threshold: 40, // Increased threshold for robust empty detection
            min_dimension: 10,
            aspect_ratio_min: 0.6,
            aspect_ratio_max: 1.6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeadAnalysis {
    pub average_color: Rgb,
    pub pixel_count: u32,
    pub variance: u32,
}

pub fn analyze_image(data: &[u8], width: usize, height: usize) -> Option<BeadAnalysis> {
    analyze_image_debug(data, width, height, None, AnalysisConfig::default())
}

pub fn analyze_image_debug(
    data: &[u8],
    width: usize,
    height: usize,
    mut mask: Option<&mut [u8]>,
    _config: AnalysisConfig,
) -> Option<BeadAnalysis> {
    if width == 0 || height == 0 || data.len() < width * height * 2 {
        return None;
    }

    if let Some(m) = &mut mask {
        m.fill(0);
    }

    // --- Background Color Estimation ---
    let mut c_r: u32 = 0;
    let mut c_g: u32 = 0;
    let mut c_b: u32 = 0;
    let mut c_cnt = 0;

    // Sample Specific Rectangle (10,3) -> (15,6)
    // User estimation: Edges are raised, this region is a better representation of the background.
    let min_bg_x = 10;
    let max_bg_x = 15;
    let min_bg_y = 3;
    let max_bg_y = 6;

    for y in min_bg_y..=max_bg_y {
        for x in min_bg_x..=max_bg_x {
            // Bounds check
            if x >= width || y >= height {
                continue;
            }

            let idx = (y * width + x) * 2;
            if idx + 1 >= data.len() {
                continue;
            }
            let p = u16::from_be_bytes([data[idx], data[idx + 1]]);
            let rgb = Rgb::from_rgb565(p);
            c_r += rgb.r as u32;
            c_g += rgb.g as u32;
            c_b += rgb.b as u32;
            c_cnt += 1;
        }
    }
    let bg_color = if c_cnt > 0 {
        Rgb {
            r: (c_r / c_cnt) as u8,
            g: (c_g / c_cnt) as u8,
            b: (c_b / c_cnt) as u8,
        }
    } else {
        Rgb { r: 0, g: 0, b: 0 }
    };

    // --- Ring Search Configuration ---
    // User Constraints:
    // x[16,24], y[16,18]
    // Ring Radii 3, 7 (Optimal Variance)
    let r_inner = 3i32;
    let r_outer = 7i32;
    let r_inner_sq = r_inner.pow(2);
    let r_outer_sq = r_outer.pow(2);

    // Constrained Search Range
    let min_cx = 16;
    let max_cx = 24; // Restored from 29
    let min_cy = 16;
    let max_cy = 18;

    let mut best_score = i64::MIN;
    let mut best_stats = None;
    let mut best_cx = (min_cx + max_cx) / 2;
    let mut best_cy = (min_cy + max_cy) / 2;

    // Scan Search Area
    for cy in min_cy..=max_cy {
        for cx in min_cx..=max_cx {
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut sum_sq_r = 0u32;
            let mut sum_sq_g = 0u32;
            let mut sum_sq_b = 0u32;
            let mut count = 0u32;

            // Scan Bounding Box of Ring
            let min_y = (cy - r_outer).max(0);
            let max_y = (cy + r_outer).min(height as i32 - 1);
            let min_x = (cx - r_outer).max(0);
            let max_x = (cx + r_outer).min(width as i32 - 1);

            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let dy = y - cy;
                    let dx = x - cx;
                    let dist_sq = dx * dx + dy * dy;

                    if dist_sq >= r_inner_sq && dist_sq <= r_outer_sq {
                        let idx = (y as usize * width + x as usize) * 2;
                        if idx + 1 >= data.len() {
                            continue;
                        }
                        let p = u16::from_be_bytes([data[idx], data[idx + 1]]);
                        let rgb = Rgb::from_rgb565(p);
                        let r = rgb.r as u32;
                        let g = rgb.g as u32;
                        let b = rgb.b as u32;

                        sum_r += r;
                        sum_g += g;
                        sum_b += b;
                        sum_sq_r += r * r;
                        sum_sq_g += g * g;
                        sum_sq_b += b * b;
                        count += 1;
                    }
                }
            }

            // count check removed to ensure we always score if possible
            if count == 0 {
                continue;
            }

            let mean_r = sum_r / count;
            let mean_g = sum_g / count;
            let mean_b = sum_b / count;

            let avg = Rgb {
                r: mean_r as u8,
                g: mean_g as u8,
                b: mean_b as u8,
            };

            // Variance Calculation
            let var_r = (sum_sq_r / count).saturating_sub(mean_r * mean_r);
            let var_g = (sum_sq_g / count).saturating_sub(mean_g * mean_g);
            let var_b = (sum_sq_b / count).saturating_sub(mean_b * mean_b);
            let total_variance = var_r + var_g + var_b;

            // Score Heuristic (Center Scoring)
            // PRIMARY: Contrast against Global BG.
            let contrast = avg.dist(&bg_color) as i64;

            // SECONDARY: Variance Penalty (/8).
            let variance_penalty = (total_variance as i64) / 8;

            let score = contrast - variance_penalty;

            if score > best_score {
                best_score = score;
                best_cx = cx;
                best_cy = cy;
                best_stats = Some((avg, count, total_variance));
            }
        }
    }

    // --- Threshold Check ---
    if best_score < -200000 {
        return None;
    }

    if let Some((avg, count, variance)) = best_stats {
        // Draw Mask for Debug
        if let Some(m) = mask {
            let cy = best_cy;
            let cx = best_cx;
            let min_y = (cy - r_outer).max(0);
            let max_y = (cy + r_outer).min(height as i32 - 1);
            let min_x = (cx - r_outer).max(0);
            let max_x = (cx + r_outer).min(width as i32 - 1);

            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let dy = y - cy;
                    let dx = x - cx;
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq >= r_inner_sq && dist_sq <= r_outer_sq {
                        let m_idx = y as usize * width + x as usize;
                        if m_idx < m.len() {
                            m[m_idx] = 1;
                        }
                    }
                }
            }
            if best_cy as usize * width + (best_cx as usize) < m.len() {
                m[best_cy as usize * width + (best_cx as usize)] = 4;
            }
        }

        return Some(BeadAnalysis {
            average_color: avg,
            pixel_count: count,
            variance: variance,
        });
    }

    None
}
