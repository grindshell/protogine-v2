//! `pg.system`.

use luars::{Lua, LuaResult, LuaTable};

use super::Module;
use crate::system;

pub fn install(lua: &mut Lua, pg: &LuaTable) -> LuaResult<()> {
    let mut m = Module::new(lua)?;
    m.function("getOS", |args| args.ret(system::os()))?;
    m.function("getProcessorCount", |args| {
        args.ret(system::processor_count() as i64)
    })?;
    m.function("getClipboardText", |args| {
        args.ret(system::clipboard_text())
    })?;
    m.function("setClipboardText", |args| {
        let text = args.bytes(1)?;
        system::set_clipboard_text(&String::from_utf8_lossy(&text));
        Ok(0)
    })?;
    m.function("getPowerInfo", |args| {
        let info = system::power_info();
        args.ret((
            info.state.name(),
            info.percent.map(i64::from),
            info.seconds.map(i64::from),
        ))
    })?;
    m.function("openURL", |args| {
        let url = args.string(1)?;
        args.ret(system::open_url(&url))
    })?;
    m.function("vibrate", |args| {
        system::vibrate(args.opt_number(1, 0.5)?);
        Ok(0)
    })?;
    // Love2D reports music from other apps on phones.
    m.function("hasBackgroundMusic", |args| args.ret(false))?;
    m.finish(pg, "system")
}
