//! Engine-owned screens: the no-game screen and the error screen.

use macroquad::{miniquad::window::clipboard_set, prelude::*};

use crate::vfs::Vfs;

const TEXT_SIZE: u16 = 16;
const MARGIN: f32 = 40.0;

/// Shown when the engine starts without a game. Returns a game dropped onto the window.
pub fn no_game_frame() -> Option<Result<Vfs, String>> {
    clear_background(Color::from_rgba(38, 42, 51, 255));

    let hint = if cfg!(target_arch = "wasm32") {
        "Drop a game .zip onto this page to run it."
    } else {
        "Run a game with `protogine-v2 <game folder or .zip>`,\n\
         or drop a game folder or .zip onto this window."
    };
    let text = format!(
        "protogine {}\n\nNo game loaded.\n\n{hint}",
        env!("CARGO_PKG_VERSION")
    );
    draw_wrapped(&text, MARGIN, MARGIN, screen_width() - MARGIN * 2.0, WHITE);

    let dropped = get_dropped_files().into_iter().next()?;
    Some(match (dropped.path, dropped.bytes) {
        (Some(path), _) if path.is_dir() => Vfs::mount_path(&path).map_err(|e| e.to_string()),
        (path, Some(bytes)) => {
            let name = path.as_ref().and_then(|p| p.file_stem()?.to_str());
            let source = path
                .as_ref()
                .map_or("dropped file".into(), |p| p.display().to_string());
            Vfs::mount_zip(&bytes, name, source).map_err(|e| e.to_string())
        }
        (Some(path), None) => Vfs::mount_path(&path).map_err(|e| e.to_string()),
        (None, None) => Err("the dropped file could not be read".to_string()),
    })
}

/// Love2D-style error screen: the message and traceback, copyable with Ctrl+C.
pub struct ErrorScreen {
    report: String,
    copied: bool,
}

impl ErrorScreen {
    pub fn new(report: String) -> Self {
        // miniquad's log macros don't support inline format arguments.
        macroquad::logging::error!("{}", report);
        // For automated tests (tests/lua.rs): exit instead of waiting on the error screen.
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::var_os("PROTOGINE_EXIT_ON_ERROR").is_some() {
            std::process::exit(1);
        }
        // On the web, gl.js can only fill the clipboard from inside the browser's own copy
        // event, which fires before the next frame, so stage the text now. (Natively this
        // would clobber the player's clipboard, so there it waits for Ctrl+C.)
        if cfg!(target_arch = "wasm32") {
            clipboard_set(&report);
        }
        ErrorScreen {
            report,
            copied: false,
        }
    }

    /// Draws one frame. Returns `true` when the player asks to quit.
    pub fn frame(&mut self) -> bool {
        let modifier = [
            KeyCode::LeftControl,
            KeyCode::RightControl,
            KeyCode::LeftSuper,
            KeyCode::RightSuper,
        ]
        .into_iter()
        .any(is_key_down);
        if modifier && is_key_pressed(KeyCode::C) {
            clipboard_set(&self.report);
            self.copied = true;
        }

        clear_background(Color::from_rgba(89, 157, 220, 255));
        let mut footer = String::from(if self.copied {
            "Copied to clipboard!"
        } else {
            "Press Ctrl+C to copy this error."
        });
        if !cfg!(target_arch = "wasm32") {
            footer.push_str(" Press Escape to quit.");
        }
        let text = format!("Error\n\n{}\n\n{footer}", self.report);
        draw_wrapped(&text, MARGIN, MARGIN, screen_width() - MARGIN * 2.0, WHITE);

        !cfg!(target_arch = "wasm32") && is_key_pressed(KeyCode::Escape)
    }
}

/// Draws `text` in the default font, wrapped to `width`, with its top-left corner at `x, y`.
fn draw_wrapped(text: &str, x: f32, y: f32, width: f32, color: Color) {
    let line_height = measure_text("Mg|", None, TEXT_SIZE, 1.0);
    let wrapped = wrap_text(text, None, TEXT_SIZE, 1.0, width.max(1.0));
    for (i, line) in wrapped.lines().enumerate() {
        let baseline = y + line_height.offset_y + i as f32 * line_height.height * 1.25;
        draw_text_ex(
            line,
            x,
            baseline,
            TextParams {
                font_size: TEXT_SIZE,
                color,
                ..Default::default()
            },
        );
    }
}
