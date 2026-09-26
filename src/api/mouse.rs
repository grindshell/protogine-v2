//! `pg.mouse`.

use luars::{Lua, LuaResult, LuaTable};
use macroquad::miniquad::CursorIcon;

use super::{Module, SharedHost, number};

/// Love2D's system cursor names. macroquad has no separate busy-with-arrow cursor, so
/// `"waitarrow"` is the plain wait cursor.
const CURSORS: &[(&str, CursorIcon)] = &[
    ("arrow", CursorIcon::Default),
    ("ibeam", CursorIcon::Text),
    ("wait", CursorIcon::Wait),
    ("waitarrow", CursorIcon::Wait),
    ("crosshair", CursorIcon::Crosshair),
    ("hand", CursorIcon::Pointer),
    ("sizeall", CursorIcon::Move),
    ("sizewe", CursorIcon::EWResize),
    ("sizens", CursorIcon::NSResize),
    ("sizenesw", CursorIcon::NESWResize),
    ("sizenwse", CursorIcon::NWSEResize),
    ("no", CursorIcon::NotAllowed),
];

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let mut m = Module::new(lua)?;
    {
        let host = host.clone();
        m.function("getPosition", move |args| {
            let pos = host.borrow().input.position();
            args.ret((number(pos.x), number(pos.y)))
        })?;
    }
    {
        let host = host.clone();
        m.function("getX", move |args| {
            let pos = host.borrow().input.position();
            args.ret(number(pos.x))
        })?;
    }
    {
        let host = host.clone();
        m.function("getY", move |args| {
            let pos = host.borrow().input.position();
            args.ret(number(pos.y))
        })?;
    }
    {
        let host = host.clone();
        m.function("isDown", move |args| {
            let mut down = false;
            for i in 1..=args.len().max(1) {
                let button = args.number(i)?;
                down |= button.fract() == 0.0 && host.borrow().input.is_button_down(button as i64);
            }
            args.ret(down)
        })?;
    }
    {
        let host = host.clone();
        m.function("setVisible", move |args| {
            host.borrow_mut().input.set_cursor_visible(args.boolean(1));
            Ok(0)
        })?;
    }
    {
        let host = host.clone();
        m.function("isVisible", move |args| {
            let visible = host.borrow().input.cursor_visible();
            args.ret(visible)
        })?;
    }
    {
        let host = host.clone();
        m.function("setRelativeMode", move |args| {
            host.borrow_mut().input.set_relative_mode(args.boolean(1));
            Ok(0)
        })?;
    }
    {
        let host = host.clone();
        m.function("getRelativeMode", move |args| {
            let relative = host.borrow().input.relative_mode();
            args.ret(relative)
        })?;
    }
    {
        let host = host.clone();
        m.function("setCursor", move |args| {
            let cursor = match args.get(1) {
                None => CursorIcon::Default,
                Some(_) => args.option(1, "cursor type", CURSORS)?,
            };
            host.borrow_mut().input.set_cursor(cursor);
            Ok(0)
        })?;
    }
    m.finish(pg, "mouse")
}
