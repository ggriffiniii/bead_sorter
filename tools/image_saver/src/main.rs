use clap::Parser;
use image::{Rgb, RgbImage};
use std::io::{self, Read, Write};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    port: String,

    #[arg(short, long, default_value_t = 115200)]
    baud: u32,
}

fn main() {
    let args = Args::parse();

    // Create images directory
    std::fs::create_dir_all("images").unwrap();

    println!("Opening {} at {} baud...", args.port, args.baud);
    let mut port = serialport::new(&args.port, args.baud)
        .timeout(Duration::from_millis(2000))
        .open()
        .expect("Failed to open unique port");

    println!("Listening for BEAD frames...");

    let mut buf = [0u8; 1];
    let mut state = 0;

    loop {
        match port.read_exact(&mut buf) {
            Ok(_) => {
                let b = buf[0];
                match state {
                    0 => {
                        if b == 0xBE {
                            state = 1;
                        } else {
                            state = 0;
                        }
                    }
                    1 => {
                        if b == 0xAD {
                            state = 2;
                        } else if b == 0xBE {
                            state = 1;
                        } else {
                            state = 0;
                        }
                    }
                    2 => {
                        if b == 0x1F {
                            state = 3;
                        } else if b == 0xBE {
                            state = 1;
                        } else {
                            state = 0;
                        }
                    }
                    3 => {
                        if b == 0x01 {
                            print!("Header found! Capturing frame... ");
                            io::stdout().flush().unwrap();

                            // Frame size: 40 * 30 * 2 = 2400 bytes
                            let mut frame_buf = [0u8; 2400];
                            if let Ok(_) = port.read_exact(&mut frame_buf) {
                                println!("RX OK.");
                                save_image(&frame_buf);
                            } else {
                                println!("Timeout reading frame data.");
                            }
                            state = 0;
                        } else if b == 0xBE {
                            state = 1;
                        } else {
                            state = 0;
                        }
                    }
                    _ => state = 0,
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::TimedOut => continue,
            Err(e) => {
                eprintln!("Serial Read Error: {:?}", e);
                break;
            }
        }
    }
}

fn save_image(data: &[u8]) {
    let width = 40;
    let height = 30;
    let mut img = RgbImage::new(width, height);

    for (i, chunk) in data.chunks(2).enumerate() {
        // User confirmed Big Endian from Camera
        let p = u16::from_be_bytes([chunk[0], chunk[1]]);

        // RGB565: RRRRR(5) GGGGGG(6) BBBBB(5)
        let r = ((p >> 11) & 0x1F) as u8;
        let g = ((p >> 5) & 0x3F) as u8;
        let b = (p & 0x1F) as u8;

        // Expand to 8-bit (Scale up)
        let r8 = ((r as u16 * 255) / 31) as u8;
        let g8 = ((g as u16 * 255) / 63) as u8;
        let b8 = ((b as u16 * 255) / 31) as u8;

        let x = (i as u32) % width;
        let y = (i as u32) / width;
        if x < width && y < height {
            img.put_pixel(x, y, Rgb([r8, g8, b8]));
        }
    }

    let timestamp = chrono::Utc::now().timestamp_millis();
    let name = format!("images/bead_{}.png", timestamp);
    match img.save(&name) {
        Ok(_) => println!("Saved: {}", name),
        Err(e) => println!("Error saving image: {}", e),
    }
}
