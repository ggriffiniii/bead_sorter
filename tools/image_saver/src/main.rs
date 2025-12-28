use clap::Parser;
use image::{Rgb, RgbImage};
use minifb::{Key, Window, WindowOptions};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    port: String,

    #[arg(short, long, default_value_t = 115200)]
    baud: u32,
}

const WIDTH: usize = 40;
const HEIGHT: usize = 30;

fn main() {
    let args = Args::parse();

    // Create images directory
    std::fs::create_dir_all("images").unwrap();

    let (tx, rx): (mpsc::Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();

    // Spawn Serial Reader Thread
    let args_clone = args.clone();
    thread::spawn(move || {
        serial_loop(args_clone, tx);
    });

    // GUI Loop
    let mut window = Window::new(
        "Bead Sorter Live View",
        WIDTH * 10,
        HEIGHT * 10,
        WindowOptions {
            resize: true,
            scale: minifb::Scale::X1,
            ..WindowOptions::default()
        },
    )
    .unwrap_or_else(|e| {
        panic!("{}", e);
    });

    // Limit to 30 fps
    window.limit_update_rate(Some(std::time::Duration::from_micros(33300)));

    let mut buffer: Vec<u32> = vec![0; WIDTH * HEIGHT];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Check for new frames
        loop {
            match rx.try_recv() {
                Ok(frame_data) => {
                    // Convert frame to ARGB buffer and save to disk
                    process_frame(&frame_data, &mut buffer);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        // Update window with latest buffer state
        // We scale manually? No, we created window size 400x300.
        // But we provide a 40x30 buffer? minifb handles scaling if we create window with larger size?
        // Actually Minifb expects buffer to match window size unless we use `update_with_buffer(&buffer, width, height)`.
        // If we pass 40,30 to update_with_buffer, minifb will scale it up to window size.
        window.update_with_buffer(&buffer, WIDTH, HEIGHT).unwrap();
    }
}

fn serial_loop(args: Args, tx: mpsc::Sender<Vec<u8>>) {
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
                            let mut frame_buf = vec![0u8; WIDTH * HEIGHT * 2];
                            if let Ok(_) = port.read_exact(&mut frame_buf) {
                                println!("RX OK.");
                                // Send to main thread
                                if let Err(_) = tx.send(frame_buf) {
                                    break;
                                }
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
                // Try to reopen? Or just break.
                // For now break, retrying logic is complex.
                break;
            }
        }
    }
}

fn process_frame(data: &[u8], buffer: &mut [u32]) {
    let width = WIDTH as u32;
    let height = HEIGHT as u32;
    let mut img = RgbImage::new(width, height);

    for (i, chunk) in data.chunks(2).enumerate() {
        if i >= buffer.len() {
            break;
        }

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

        // Update display buffer (0x00RRGGBB)
        buffer[i] = ((r8 as u32) << 16) | ((g8 as u32) << 8) | (b8 as u32);

        // Update image for saving
        let x = (i as u32) % width;
        let y = (i as u32) / width;
        if x < width && y < height {
            img.put_pixel(x, y, Rgb([r8, g8, b8]));
        }
    }

    // Save to disk
    let timestamp = chrono::Utc::now().timestamp_millis();
    let name = format!("images/bead_{}.png", timestamp);
    match img.save(&name) {
        Ok(_) => println!("Saved: {}", name),
        Err(e) => println!("Error saving image: {}", e),
    }
}
