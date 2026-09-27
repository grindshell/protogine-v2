//! A running game: its Lua state, the host state `pg.*` functions share, and the lifecycle
//! (`conf.lua`, `main.lua`, and the `pg.*` callbacks).

use std::{cell::RefCell, rc::Rc};

use luars::{IntoLua, Lua, LuaApi, LuaError, LuaFunction, LuaTable, SafeOption, Stdlib, Value};
use macroquad::{
    input::mouse_position,
    math::Vec2,
    time::get_time,
    window::{screen_height, screen_width},
};

use crate::{
    api::{self, SharedHost},
    audio::{Audio, SharedAudio},
    conf::Conf,
    filesystem::Filesystem,
    graphics::Graphics,
    input::{Event, Input, RawEvent},
    vfs::Vfs,
};

/// Engine state shared by the `pg.*` functions.
pub struct Host {
    /// The game's files and its save directory.
    pub fs: Filesystem,
    /// `None` until the window exists; `pg.graphics` is only installed after that.
    pub graphics: Option<Graphics>,
    pub input: Input,
    pub audio: SharedAudio,
    pub delta: f64,
    pub fullscreen: bool,
    pub quit_requested: bool,
}

impl Host {
    pub fn graphics(&mut self) -> &mut Graphics {
        self.graphics
            .as_mut()
            .expect("pg.graphics is only installed once the window exists")
    }
}

pub struct Game {
    // Lua handles point into the VM, so they must drop before `lua`.
    /// The prelude's `invoke(f, ...)`: `xpcall` with a traceback handler.
    invoke: LuaFunction,
    pg: LuaTable,
    host: SharedHost,
    conf: Conf,
    /// Whether `start` ran, so the window exists.
    started: bool,
    size: (f32, f32),
    lua: Lua,
}

impl Game {
    /// Creates the game's Lua state and runs `conf.lua`. Doesn't need the window.
    pub fn new(vfs: Vfs) -> Result<Game, String> {
        let mut lua = Lua::new(SafeOption::default());
        let host = Rc::new(RefCell::new(Host {
            fs: Filesystem::new(vfs),
            graphics: None,
            input: Input::default(),
            audio: Audio::new(),
            delta: 0.0,
            fullscreen: false,
            quit_requested: false,
        }));

        let (invoke, pg) = match run_prelude(&mut lua, &host) {
            Ok(handles) => handles,
            Err(e) => return Err(lua.get_error_message(e).message),
        };
        let mut game = Game {
            invoke,
            pg,
            host,
            conf: Conf::default(),
            started: false,
            size: (0.0, 0.0),
            lua,
        };
        game.conf = game.run_conf()?;
        {
            let mut host = game.host.borrow_mut();
            let fs = &mut host.fs;
            let identity = game.conf.identity.clone();
            let identity = identity.unwrap_or_else(|| fs.game_name().to_string());
            fs.set_identity(&identity, game.conf.append_identity)
                .map_err(|e| format!("conf.lua: t.identity: {e}"))?;
        }
        Ok(game)
    }

    pub fn conf(&self) -> &Conf {
        &self.conf
    }

    /// Installs the `pg.*` modules, runs `main.lua`, and calls `pg.load(args)`. Needs the
    /// window.
    pub fn start(&mut self, args: &[String]) -> Result<(), String> {
        {
            let mut host = self.host.borrow_mut();
            host.graphics = Some(Graphics::new());
            host.input = Input::new(mouse_position().into());
            host.fullscreen = self.conf.window.fullscreen;
        }
        self.started = true;
        self.size = (screen_width(), screen_height());

        let installed = api::install(&mut self.lua, &self.pg, &self.host);
        installed.map_err(|e| self.lua_error(e))?;

        let dofile = self.global_function("dofile")?;
        let main = self.pack("main.lua")?;
        self.call(&dofile, vec![main])?;

        if let Some(load) = self.callback("load")? {
            let args = self
                .lua
                .create_sequence_from(args.iter().cloned())
                .map_err(|e| self.lua_error(e))?;
            let args = self.pack(args)?;
            self.call(&load, vec![args])?;
        }
        Ok(())
    }

    /// Runs one frame: window and input events, `pg.update(dt)`, then `pg.draw()`.
    pub fn frame(&mut self, dt: f64, events: Vec<RawEvent>) -> Result<(), String> {
        let dt = dt.min(self.conf.maxdelta);
        self.host.borrow_mut().delta = dt;

        self.dispatch_events(events)?;

        if let Some(update) = self.callback("update")? {
            let dt = self.pack(dt)?;
            self.call(&update, vec![dt])?;
        }

        self.host.borrow_mut().graphics().begin_frame();
        if let Some(draw) = self.callback("draw")? {
            self.call(&draw, Vec::new())?;
            if self.host.borrow_mut().graphics().canvas().is_some() {
                return Err("a Canvas was still active when pg.draw returned (call \
                            pg.graphics.setCanvas() to draw to the screen again)"
                    .to_string());
            }
        }
        Ok(())
    }

    /// Whether the game called `pg.event.quit()` since the last check.
    pub fn take_quit_request(&mut self) -> bool {
        std::mem::take(&mut self.host.borrow_mut().quit_requested)
    }

    /// Calls `pg.quit()`. Returns `true` if the game canceled the quit.
    pub fn quit(&mut self) -> Result<bool, String> {
        let Some(quit) = self.callback("quit")? else {
            return Ok(false);
        };
        let canceled = self.call(&quit, Vec::new())?;
        Ok(canceled.get::<bool>().unwrap_or(false))
    }

    fn run_conf(&mut self) -> Result<Conf, String> {
        let mut conf = Conf::default();
        let exists = self.host.borrow().fs.exists("conf.lua");
        if !exists {
            return Ok(conf);
        }

        let dofile = self.global_function("dofile")?;
        let path = self.pack("conf.lua")?;
        self.call(&dofile, vec![path])?;
        let Some(conf_fn) = self.callback("conf")? else {
            return Ok(conf);
        };

        let t = self.defaults_table(&conf).map_err(|e| self.lua_error(e))?;
        let arg = self.pack(t.clone())?;
        self.call(&conf_fn, vec![arg])?;
        read_conf(&t, &mut conf).map_err(|e| format!("conf.lua: {e}"))?;
        Ok(conf)
    }

    fn defaults_table(&mut self, conf: &Conf) -> Result<LuaTable, LuaError> {
        let w = &conf.window;
        let window = self.lua.create_table()?;
        window.set("title", w.title.as_str())?;
        window.set("width", i64::from(w.width))?;
        window.set("height", i64::from(w.height))?;
        window.set("resizable", w.resizable)?;
        window.set("fullscreen", w.fullscreen)?;
        window.set("highdpi", w.highdpi)?;
        window.set("msaa", i64::from(w.msaa))?;
        window.set("vsync", w.vsync)?;
        let t = self.lua.create_table()?;
        t.set("window", window)?;
        t.set("maxdelta", conf.maxdelta)?;
        t.set("identity", conf.identity.as_deref())?;
        t.set("appendidentity", conf.append_identity)?;
        Ok(t)
    }

    /// Calls `pg.resize` if the window changed size, then the callbacks for this frame's input
    /// and focus events, in order.
    fn dispatch_events(&mut self, events: Vec<RawEvent>) -> Result<(), String> {
        let size = (screen_width(), screen_height());
        if size != self.size {
            self.size = size;
            if let Some(resize) = self.callback("resize")? {
                let args = vec![self.number(size.0)?, self.number(size.1)?];
                self.call(&resize, args)?;
            }
        }

        let events = self.host.borrow_mut().input.process(events, get_time());
        for event in events {
            if let Some(callback) = self.callback(event.callback())? {
                let args = self.event_args(event)?;
                self.call(&callback, args)?;
            }
        }
        Ok(())
    }

    /// The callback arguments for an input event, following Love2D's signatures.
    fn event_args(&mut self, event: Event) -> Result<Vec<Value>, String> {
        let point = |game: &mut Game, pos: Vec2| -> Result<[Value; 2], String> {
            Ok([game.number(pos.x)?, game.number(pos.y)?])
        };
        Ok(match event {
            // Key names are physical keys, so they double as Love2D's scancode argument.
            Event::KeyPressed { key, repeat } => {
                vec![self.pack(key)?, self.pack(key)?, self.pack(repeat)?]
            }
            Event::KeyReleased { key } => vec![self.pack(key)?, self.pack(key)?],
            Event::TextInput(c) => vec![self.pack(c.to_string())?],
            Event::MousePressed(click) | Event::MouseReleased(click) => {
                let mut args = point(self, click.pos)?.to_vec();
                args.push(self.pack(i64::from(click.button))?);
                args.push(self.pack(click.touch)?);
                args.push(self.pack(i64::from(click.presses))?);
                args
            }
            Event::MouseMoved { pos, delta, touch } => {
                let mut args = point(self, pos)?.to_vec();
                args.extend(point(self, delta)?);
                args.push(self.pack(touch)?);
                args
            }
            Event::WheelMoved(delta) => point(self, delta)?.to_vec(),
            Event::TouchPressed(t) | Event::TouchMoved(t) | Event::TouchReleased(t) => {
                let mut args = vec![self.pack(t.id as i64)?];
                args.extend(point(self, t.pos)?);
                args.extend(point(self, t.delta)?);
                args.push(self.pack(1_i64)?);
                args
            }
            Event::Visible(visible) => vec![self.pack(visible)?],
        })
    }

    /// `pg.<name>`, if the game defined it.
    fn callback(&mut self, name: &str) -> Result<Option<LuaFunction>, String> {
        let value: Value = self.pg.get(name).map_err(|e| self.lua_error(e))?;
        if value.is_nil() {
            return Ok(None);
        }
        match value.as_function() {
            Some(function) => Ok(Some(function)),
            None => Err(format!(
                "pg.{name} must be a function, not a {}",
                value.type_name()
            )),
        }
    }

    fn global_function(&mut self, name: &str) -> Result<LuaFunction, String> {
        self.lua
            .globals()
            .get::<LuaFunction>(name)
            .map_err(|e| self.lua_error(e))
    }

    /// Calls `f(args...)` through `invoke`. On error, returns the message and traceback.
    fn call(&mut self, f: &LuaFunction, args: Vec<Value>) -> Result<Value, String> {
        let mut invoke_args = vec![self.pack(f.clone())?];
        invoke_args.extend(args);
        match self.invoke.call::<_, (bool, Value)>(invoke_args) {
            Ok((true, value)) => Ok(value),
            Ok((false, error)) => Err(error.as_string().unwrap_or_else(|| error.to_string_lossy())),
            Err(e) => Err(self.lua_error(e)),
        }
    }

    fn pack(&mut self, value: impl IntoLua) -> Result<Value, String> {
        self.lua.pack(value).map_err(|e| self.lua_error(e))
    }

    /// Packs a number the way `pg.*` functions return them (see [`api::number`]).
    fn number(&mut self, n: f32) -> Result<Value, String> {
        self.pack(api::number(n))
    }

    fn lua_error(&mut self, error: LuaError) -> String {
        self.lua.get_error_message(error).message
    }
}

impl Drop for Game {
    /// Silences the game and gives the cursor and the screen back to the engine's screens when
    /// it stops.
    fn drop(&mut self) {
        let mut host = self.host.borrow_mut();
        host.audio.borrow_mut().shutdown();
        if self.started {
            host.input.restore_cursor();
            host.graphics().shutdown();
        }
    }
}

/// Opens the standard library, runs the prelude, and returns `invoke` and the `pg` table.
fn run_prelude(lua: &mut Lua, host: &SharedHost) -> Result<(LuaFunction, LuaTable), LuaError> {
    lua.open_stdlibs(&[
        Stdlib::Basic,
        Stdlib::String,
        Stdlib::Table,
        Stdlib::Math,
        Stdlib::Utf8,
        Stdlib::Coroutine,
        Stdlib::Os,
        Stdlib::Debug,
    ])?;

    let read_source = {
        let host = host.clone();
        lua.create_function(move |path: String| -> (Option<String>, Option<String>) {
            match host.borrow().fs.read_string(&path) {
                Ok(source) => (Some(source), None),
                Err(e) => (None, Some(e.to_string())),
            }
        })?
    };
    let log = lua.create_function(|message: String| log_line(&message))?;

    let invoke: LuaFunction = lua
        .load(api::PRELUDE)
        .set_name("=prelude")
        .call1((read_source, log))?;
    let pg: LuaTable = lua.globals().get("pg")?;
    Ok((invoke, pg))
}

/// Where Lua's `print` goes: stdout natively, the browser console on the web.
fn log_line(message: &str) {
    if cfg!(target_arch = "wasm32") {
        // miniquad's log macros don't support inline format arguments.
        macroquad::logging::info!("{}", message);
    } else {
        println!("{message}");
    }
}

/// Reads the table passed to `pg.conf(t)` back into `conf`.
fn read_conf(t: &LuaTable, conf: &mut Conf) -> Result<(), String> {
    let window: LuaTable = t
        .get("window")
        .map_err(|_| "t.window must be a table".to_string())?;
    let w = &mut conf.window;
    w.title = field(&window, "window.title")?;
    w.width = field::<i64>(&window, "window.width")? as i32;
    w.height = field::<i64>(&window, "window.height")? as i32;
    w.resizable = field(&window, "window.resizable")?;
    w.fullscreen = field(&window, "window.fullscreen")?;
    w.highdpi = field(&window, "window.highdpi")?;
    w.msaa = field::<i64>(&window, "window.msaa")? as i32;
    w.vsync = field(&window, "window.vsync")?;
    conf.maxdelta = field(t, "maxdelta")?;
    conf.identity = field(t, "identity")?;
    conf.append_identity = field(t, "appendidentity")?;
    Ok(())
}

fn field<T: luars::FromLua>(table: &LuaTable, path: &str) -> Result<T, String> {
    let key = path.rsplit('.').next().unwrap_or(path);
    table.get(key).map_err(|_| {
        let got = table.get::<Value>(key).map_or("unknown", |v| v.type_name());
        format!("t.{path} has the wrong type ({got})")
    })
}
