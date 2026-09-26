//! `pg.window`.

use luars::{Lua, LuaResult, LuaTable};
use macroquad::window::{request_new_screen_size, screen_dpi_scale, set_fullscreen};

use super::{Module, SharedHost, number};

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let mut m = Module::new(lua)?;
    {
        let host = host.clone();
        m.function("setFullscreen", move |args| {
            let fullscreen = args.boolean(1);
            set_fullscreen(fullscreen);
            host.borrow_mut().fullscreen = fullscreen;
            Ok(0)
        })?;
    }
    {
        // macroquad has no getter, so this reports the last requested state.
        let host = host.clone();
        m.function("getFullscreen", move |args| {
            let fullscreen = host.borrow().fullscreen;
            args.ret(fullscreen)
        })?;
    }
    m.function("setMode", |args| {
        let (width, height) = (args.f32(1)?, args.f32(2)?);
        if !cfg!(target_arch = "wasm32") {
            request_new_screen_size(width, height);
        }
        Ok(0)
    })?;
    m.function("getDPIScale", |args| args.ret(number(screen_dpi_scale())))?;
    m.finish(pg, "window")
}
