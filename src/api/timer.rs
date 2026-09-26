//! `pg.timer`.

use luars::{Lua, LuaResult, LuaTable};
use macroquad::time::{get_fps, get_time};

use super::{Module, SharedHost};

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let mut m = Module::new(lua)?;
    {
        let host = host.clone();
        m.function("getDelta", move |args| {
            let delta = host.borrow().delta;
            args.ret(delta)
        })?;
    }
    m.function("getFPS", |args| args.ret(i64::from(get_fps())))?;
    m.function("getTime", |args| args.ret(get_time()))?;
    m.finish(pg, "timer")
}
