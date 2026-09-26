mod api;
mod audio;
mod conf;
mod engine;
mod game;
mod graphics;
mod input;
mod math;
mod screens;
mod vfs;

use conf::Conf;
use engine::Start;
use macroquad::Window;

/// The archive the web build fetches on startup, written by `cargo xtask web --game`.
#[cfg(target_arch = "wasm32")]
const GAME_ARCHIVE: &str = "game.zip";

/// Native: mount the game and run its `conf.lua` before opening the window, so the window
/// can be created with the game's settings.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut args = std::env::args().skip(1);
    let start = match args.next() {
        None => Start::NoGame,
        Some(path) => match vfs::Vfs::mount_path(std::path::Path::new(&path))
            .map_err(|e| e.to_string())
            .and_then(game::Game::new)
        {
            Ok(game) => Start::Game(game),
            Err(report) => Start::Error(report),
        },
    };
    let conf = match &start {
        Start::Game(game) => game.conf().clone(),
        _ => Conf::default(),
    };
    let args: Vec<String> = args.collect();
    Window::from_config(conf.window_conf(), engine::run(start, args));
}

/// Web: the page owns the canvas, so open with defaults, then fetch and mount the game.
#[cfg(target_arch = "wasm32")]
fn main() {
    Window::from_config(Conf::default().window_conf(), async {
        let start = match macroquad::file::load_file(GAME_ARCHIVE).await {
            Err(_) => Start::NoGame,
            Ok(bytes) => match vfs::Vfs::mount_zip(&bytes)
                .map_err(|e| e.to_string())
                .and_then(game::Game::new)
            {
                Ok(game) => Start::Game(game),
                Err(report) => Start::Error(report),
            },
        };
        engine::run(start, Vec::new()).await;
    });
}
