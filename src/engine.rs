//! The main loop: runs the current screen (no game, a game, its error screen, or blank) and
//! moves between them.

use macroquad::prelude::*;

use crate::{
    game::Game,
    screens::{self, ErrorScreen},
    vfs::Vfs,
};

/// What the engine starts with, decided before the window opens (native) or after fetching
/// the game archive (web).
pub enum Start {
    NoGame,
    /// A game whose `conf.lua` has already run.
    Game(Game),
    Error(String),
}

enum Screen {
    NoGame,
    Game(Game),
    Error(ErrorScreen),
    /// After a web game quits: there's no way to close a browser tab, so show nothing.
    Blank,
}

enum Flow {
    Continue,
    Exit,
}

pub async fn run(start: Start, args: Vec<String>) {
    prevent_quit();

    let mut screen = match start {
        Start::NoGame => Screen::NoGame,
        Start::Game(game) => start_game(game, &args),
        Start::Error(report) => Screen::Error(ErrorScreen::new(report)),
    };

    loop {
        let (next, flow) = frame(screen, &args);
        screen = next;
        if let Flow::Exit = flow {
            macroquad::miniquad::window::order_quit();
            return;
        }
        next_frame().await;
    }
}

fn frame(screen: Screen, args: &[String]) -> (Screen, Flow) {
    match screen {
        Screen::NoGame => match screens::no_game_frame() {
            None if is_quit_requested() => (Screen::NoGame, Flow::Exit),
            None => (Screen::NoGame, Flow::Continue),
            Some(mounted) => (load_dropped(mounted, args), Flow::Continue),
        },
        Screen::Game(mut game) => {
            if let Err(report) = game.frame(get_frame_time() as f64) {
                return (Screen::Error(ErrorScreen::new(report)), Flow::Continue);
            }
            if !(game.take_quit_request() || is_quit_requested()) {
                return (Screen::Game(game), Flow::Continue);
            }
            match game.quit() {
                Ok(true) => (Screen::Game(game), Flow::Continue),
                Ok(false) if cfg!(target_arch = "wasm32") => (Screen::Blank, Flow::Continue),
                Ok(false) => (Screen::Blank, Flow::Exit),
                Err(report) => (Screen::Error(ErrorScreen::new(report)), Flow::Continue),
            }
        }
        Screen::Error(mut error) => {
            let exit = error.frame() || is_quit_requested();
            (
                Screen::Error(error),
                if exit { Flow::Exit } else { Flow::Continue },
            )
        }
        Screen::Blank => {
            clear_background(BLACK);
            let flow = if is_quit_requested() {
                Flow::Exit
            } else {
                Flow::Continue
            };
            (Screen::Blank, flow)
        }
    }
}

/// Starts a game dropped onto the no-game screen. Its window settings can only be applied
/// partially, since the window already exists.
fn load_dropped(mounted: Result<Vfs, String>, args: &[String]) -> Screen {
    let game = match mounted.and_then(Game::new) {
        Ok(game) => game,
        Err(report) => return Screen::Error(ErrorScreen::new(report)),
    };
    if !cfg!(target_arch = "wasm32") {
        let window = &game.conf().window;
        set_fullscreen(window.fullscreen);
        if !window.fullscreen {
            request_new_screen_size(window.width as f32, window.height as f32);
        }
    }
    start_game(game, args)
}

fn start_game(mut game: Game, args: &[String]) -> Screen {
    match game.start(args) {
        Ok(()) => Screen::Game(game),
        Err(report) => Screen::Error(ErrorScreen::new(report)),
    }
}
