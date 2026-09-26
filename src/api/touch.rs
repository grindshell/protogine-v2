//! `pg.touch`.

use luars::{Lua, LuaApi, LuaResult, LuaTable};

use super::{Module, SharedHost, number};

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let mut m = Module::new(lua)?;
    {
        let host = host.clone();
        m.function("getTouches", move |args| {
            let ids = host.borrow().input.touch_ids();
            let ids = args
                .state
                .create_sequence_from(ids.into_iter().map(|id| id as i64))?;
            args.ret(ids)
        })?;
    }
    {
        let host = host.clone();
        m.function("getPosition", move |args| {
            let id = args.number(1)?;
            let pos = host.borrow().input.touch_position(id as u64);
            match pos {
                Some(pos) if id.fract() == 0.0 && id >= 0.0 => {
                    args.ret((number(pos.x), number(pos.y)))
                }
                _ => Err(args.arg_error(1, format!("no active touch with id {id}"))),
            }
        })?;
    }
    m.finish(pg, "touch")
}
