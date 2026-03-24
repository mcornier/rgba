use rgba_core::types::{GbaButton, SpeedMode};
use rgba_gba::Gba;
use std::time::{Duration, Instant};

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: rgba-frontend <rom.gba> [bios.bin]");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --scale <N>    Window scale factor (default: 3)");
        eprintln!("  --fast         Start in fast-forward mode");
        eprintln!("  --headless     Run without display (benchmarking)");
        std::process::exit(1);
    }

    let rom_path = &args[1];
    let bios_path = args.iter().position(|a| !a.starts_with('-') && a != rom_path && a != &args[0])
        .map(|i| args[i].clone());

    let scale = args.iter().position(|a| a == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3);
    let fast = args.iter().any(|a| a == "--fast");
    let headless = args.iter().any(|a| a == "--headless");

    // Load ROM
    let rom_data = std::fs::read(rom_path).unwrap_or_else(|e| {
        eprintln!("Failed to load ROM '{}': {}", rom_path, e);
        std::process::exit(1);
    });

    let mut gba = Gba::new();

    // Load BIOS if provided
    if let Some(bios) = &bios_path {
        match std::fs::read(bios) {
            Ok(data) => {
                gba.load_bios(&data);
                log::info!("BIOS loaded from {}", bios);
            }
            Err(e) => log::warn!("Failed to load BIOS '{}': {}", bios, e),
        }
    }

    gba.load_rom(&rom_data);
    gba.reset();

    if fast {
        gba.set_speed(SpeedMode::Unlimited);
    }

    log::info!("ROM loaded: {} bytes", rom_data.len());
    if let Some(cart) = &gba.cartridge {
        log::info!("Title: {}", cart.title);
        log::info!("Game code: {}", cart.game_code);
        log::info!("Backup type: {:?}", cart.backup_type);
    }

    if headless {
        run_headless(&mut gba);
    } else {
        log::info!("Display mode: {}x scale", scale);
        log::info!("Controls: Arrow keys, Z=A, X=B, Enter=Start, Shift=Select, A=L, S=R");
        log::info!("  Space=Fast-forward, F1=Save state, F2=Load state");
        log::info!("No display backend compiled — running headless benchmark instead.");
        log::info!("(Add minifb/SDL2/winit dependency for windowed mode)");
        run_headless(&mut gba);
    }
}

fn run_headless(gba: &mut Gba) {
    log::info!("Running headless benchmark (1000 frames)...");
    let start = Instant::now();
    let target_frames = 1000;

    for _ in 0..target_frames {
        gba.run_frame();
    }

    let elapsed = start.elapsed();
    let fps = target_frames as f64 / elapsed.as_secs_f64();
    let speed_percent = fps / 59.7275 * 100.0;

    log::info!(
        "Benchmark: {} frames in {:.2}s = {:.1} fps ({:.1}% speed)",
        target_frames,
        elapsed.as_secs_f64(),
        fps,
        speed_percent
    );
}
