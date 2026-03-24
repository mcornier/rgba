// =============================================================================
// WebAssembly bindings for rgba GBA emulator
// =============================================================================
//
// This module provides a C-compatible FFI interface for WebAssembly.
// It can be used from JavaScript via wasm-bindgen or direct instantiation.
//
// Build with: cargo build --target wasm32-unknown-unknown -p rgba-wasm --release
//
// For wasm-bindgen, add the dependency and use #[wasm_bindgen] attributes.
// For now, we provide a raw FFI that works with any WASM runtime.

use rgba_core::types::{GbaButton, SpeedMode, AccessWidth};
use rgba_gba::Gba;

use std::cell::RefCell;

thread_local! {
    static GBA: RefCell<Option<Gba>> = RefCell::new(None);
}

fn with_gba<F, R>(f: F) -> R
where
    F: FnOnce(&mut Gba) -> R,
    R: Default,
{
    GBA.with(|gba| {
        if let Some(gba) = gba.borrow_mut().as_mut() {
            f(gba)
        } else {
            R::default()
        }
    })
}

// =============================================================================
// Initialization
// =============================================================================

/// Initialize the emulator
#[no_mangle]
pub extern "C" fn rgba_init() {
    GBA.with(|gba| {
        *gba.borrow_mut() = Some(Gba::new());
    });
}

/// Load a ROM from a pointer + length
///
/// # Safety
/// `ptr` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn rgba_load_rom(ptr: *const u8, len: usize) {
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    with_gba(|gba| {
        gba.load_rom(data);
        gba.reset();
    });
}

/// Load BIOS from a pointer + length
///
/// # Safety
/// `ptr` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn rgba_load_bios(ptr: *const u8, len: usize) {
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    with_gba(|gba| {
        gba.load_bios(data);
    });
}

// =============================================================================
// Emulation
// =============================================================================

/// Run one frame of emulation
#[no_mangle]
pub extern "C" fn rgba_run_frame() {
    with_gba(|gba| gba.run_frame());
}

/// Get pointer to the RGBA8888 framebuffer (240*160*4 bytes)
/// Returns a pointer that remains valid until the next rgba_run_frame call.
#[no_mangle]
pub extern "C" fn rgba_get_framebuffer() -> *const u8 {
    // Store the RGBA buffer in a thread-local to keep it alive
    thread_local! {
        static FB: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    }

    GBA.with(|gba| {
        if let Some(gba) = gba.borrow().as_ref() {
            FB.with(|fb| {
                let mut fb = fb.borrow_mut();
                *fb = gba.ppu.framebuffer_rgba();
                fb.as_ptr()
            })
        } else {
            std::ptr::null()
        }
    })
}

/// Get the framebuffer size in bytes (always 240*160*4 = 153600)
#[no_mangle]
pub extern "C" fn rgba_framebuffer_size() -> usize {
    240 * 160 * 4
}

// =============================================================================
// Input
// =============================================================================

/// Set key input state (active-low bitmask, same format as KEYINPUT)
#[no_mangle]
pub extern "C" fn rgba_set_keyinput(value: u16) {
    with_gba(|gba| gba.set_keyinput(value));
}

/// Press a button (0=A, 1=B, 2=Select, 3=Start, 4=Right, 5=Left, 6=Up, 7=Down, 8=R, 9=L)
#[no_mangle]
pub extern "C" fn rgba_press_button(button: u32) {
    if let Some(btn) = button_from_index(button) {
        with_gba(|gba| gba.press_button(btn));
    }
}

/// Release a button
#[no_mangle]
pub extern "C" fn rgba_release_button(button: u32) {
    if let Some(btn) = button_from_index(button) {
        with_gba(|gba| gba.release_button(btn));
    }
}

fn button_from_index(index: u32) -> Option<GbaButton> {
    match index {
        0 => Some(GbaButton::A),
        1 => Some(GbaButton::B),
        2 => Some(GbaButton::Select),
        3 => Some(GbaButton::Start),
        4 => Some(GbaButton::Right),
        5 => Some(GbaButton::Left),
        6 => Some(GbaButton::Up),
        7 => Some(GbaButton::Down),
        8 => Some(GbaButton::R),
        9 => Some(GbaButton::L),
        _ => None,
    }
}

// =============================================================================
// Audio
// =============================================================================

/// Get the number of pending audio samples
#[no_mangle]
pub extern "C" fn rgba_audio_samples_available() -> usize {
    GBA.with(|gba| {
        gba.borrow()
            .as_ref()
            .map(|g| g.apu.sample_buffer.len())
            .unwrap_or(0)
    })
}

/// Drain audio samples into a buffer (interleaved stereo f32)
/// Returns number of samples (pairs) written
///
/// # Safety
/// `ptr` must point to a buffer of at least `max_samples * 2` f32 values.
#[no_mangle]
pub unsafe extern "C" fn rgba_drain_audio(ptr: *mut f32, max_samples: usize) -> usize {
    with_gba(|gba| {
        let count = gba.apu.sample_buffer.len().min(max_samples);
        let buf = unsafe { std::slice::from_raw_parts_mut(ptr, count * 2) };
        for (i, (l, r)) in gba.apu.sample_buffer.drain(..count).enumerate() {
            buf[i * 2] = l;
            buf[i * 2 + 1] = r;
        }
        count
    })
}

// =============================================================================
// Savestates
// =============================================================================

/// Save state to a buffer allocated by the emulator.
/// Returns the length. Call rgba_get_savestate_ptr() to get the pointer.
#[no_mangle]
pub extern "C" fn rgba_save_state() -> usize {
    thread_local! {
        static STATE: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    }

    GBA.with(|gba| {
        if let Some(gba) = gba.borrow().as_ref() {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                *state = gba.save_state();
                state.len()
            })
        } else {
            0
        }
    })
}

/// Load state from a buffer
///
/// # Safety
/// `ptr` must point to `len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn rgba_load_state(ptr: *const u8, len: usize) -> bool {
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    with_gba(|gba| gba.load_state(data).is_ok())
}

// =============================================================================
// Memory inspection
// =============================================================================

/// Read a byte from any address
#[no_mangle]
pub extern "C" fn rgba_peek(address: u32) -> u8 {
    with_gba(|gba| gba.peek(address))
}

/// Read a 16-bit value
#[no_mangle]
pub extern "C" fn rgba_peek16(address: u32) -> u16 {
    with_gba(|gba| gba.peek16(address))
}

/// Read a 32-bit value
#[no_mangle]
pub extern "C" fn rgba_peek32(address: u32) -> u32 {
    with_gba(|gba| gba.peek32(address))
}

/// Write a byte to any writable address
#[no_mangle]
pub extern "C" fn rgba_poke(address: u32, value: u8) {
    with_gba(|gba| gba.poke(address, value));
}

/// Write a 16-bit value
#[no_mangle]
pub extern "C" fn rgba_poke16(address: u32, value: u16) {
    with_gba(|gba| gba.poke16(address, value));
}

/// Write a 32-bit value
#[no_mangle]
pub extern "C" fn rgba_poke32(address: u32, value: u32) {
    with_gba(|gba| gba.poke32(address, value));
}

// =============================================================================
// Speed control
// =============================================================================

/// Set speed mode: 0=Normal, 1=FastForward(2x), 2=Unlimited, 3=Paused
#[no_mangle]
pub extern "C" fn rgba_set_speed(mode: u32) {
    let speed = match mode {
        0 => SpeedMode::Normal,
        1 => SpeedMode::FastForward(2.0),
        2 => SpeedMode::Unlimited,
        3 => SpeedMode::Paused,
        _ => SpeedMode::Normal,
    };
    with_gba(|gba| gba.set_speed(speed));
}

/// Get the current frame count
#[no_mangle]
pub extern "C" fn rgba_frame_count() -> u64 {
    GBA.with(|gba| {
        gba.borrow()
            .as_ref()
            .map(|g| g.frame_count)
            .unwrap_or(0)
    })
}
