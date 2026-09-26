//! `pg.keyboard`.

use luars::{Lua, LuaResult, LuaTable};

use super::{Module, SharedHost};
use crate::input;

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let mut m = Module::new(lua)?;
    {
        let host = host.clone();
        m.function("isDown", move |args| {
            // Every argument is checked, so a typo errors even when an earlier key is down.
            let mut down = false;
            for i in 1..=args.len().max(1) {
                let name = args.string(i)?;
                match input::key_code(&name) {
                    Ok(Some(key)) => down |= host.borrow().input.is_key_down(key),
                    Ok(None) => {}
                    Err(()) => {
                        return Err(args.arg_error(i, format!("invalid key constant '{name}'")));
                    }
                }
            }
            args.ret(down)
        })?;
    }
    {
        let host = host.clone();
        m.function("setKeyRepeat", move |args| {
            host.borrow_mut().input.key_repeat = args.boolean(1);
            Ok(0)
        })?;
    }
    {
        let host = host.clone();
        m.function("hasKeyRepeat", move |args| {
            let enabled = host.borrow().input.key_repeat;
            args.ret(enabled)
        })?;
    }
    {
        let host = host.clone();
        m.function("setTextInput", move |args| {
            host.borrow_mut().input.set_text_input(args.boolean(1));
            Ok(0)
        })?;
    }
    {
        let host = host.clone();
        m.function("hasTextInput", move |args| {
            let enabled = host.borrow().input.text_input();
            args.ret(enabled)
        })?;
    }
    m.finish(pg, "keyboard")
}
