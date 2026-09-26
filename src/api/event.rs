//! `pg.event`.

use luars::{Lua, LuaResult, LuaTable};

use super::{Module, SharedHost};

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let mut m = Module::new(lua)?;
    {
        // The engine acts on the request after the current frame, via `pg.quit`.
        let host = host.clone();
        m.function("quit", move |_args| {
            host.borrow_mut().quit_requested = true;
            Ok(0)
        })?;
    }
    m.finish(pg, "event")
}
