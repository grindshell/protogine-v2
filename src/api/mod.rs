//! The Lua-facing `pg` API. Each submodule installs one `pg.*` table; docs/api.md is the spec.
//!
//! Functions are raw luars callbacks wrapped in [`Args`], which gives Love2D-style argument
//! errors (`bad argument #2 to 'rectangle' (number expected, got nil)`) and variadic arguments,
//! neither of which luars' typed callbacks support.

mod audio;
mod event;
mod graphics;
mod keyboard;
mod mouse;
mod timer;
mod touch;
mod window;

use std::{cell::RefCell, fmt::Display, rc::Rc};

use luars::{IntoLua, Lua, LuaApi, LuaError, LuaResult, LuaState, LuaTable, LuaValue};
use macroquad::math::{Vec2, vec2};

use crate::game::Host;

pub type SharedHost = Rc<RefCell<Host>>;

/// Lua run once per game before `conf.lua`; see the file for what it sets up.
pub const PRELUDE: &str = include_str!("prelude.lua");

/// Installs every `pg.*` module into the game's `pg` table. Needs the window to exist.
pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    graphics::install(lua, pg, host)?;
    timer::install(lua, pg, host)?;
    window::install(lua, pg, host)?;
    event::install(lua, pg, host)?;
    keyboard::install(lua, pg, host)?;
    mouse::install(lua, pg, host)?;
    touch::install(lua, pg, host)?;
    audio::install(lua, pg, host)?;
    Ok(())
}

/// Builds one `pg.*` table.
struct Module<'a> {
    lua: &'a mut Lua,
    table: LuaTable,
}

impl<'a> Module<'a> {
    fn new(lua: &'a mut Lua) -> LuaResult<Self> {
        let table = lua.create_table()?;
        Ok(Module { lua, table })
    }

    fn function(
        &mut self,
        name: &'static str,
        f: impl Fn(&mut Args) -> LuaResult<usize> + 'static,
    ) -> LuaResult<()> {
        let closure = self
            .lua
            .global_state_mut()
            .create_closure(move |state: &mut LuaState| f(&mut Args { state, name }))?;
        self.table.set(name, closure)
    }

    fn finish(self, pg: &LuaTable, name: &str) -> LuaResult<()> {
        pg.set(name, self.table)
    }
}

/// The arguments of a `pg.*` call, with checked accessors. Indices are 1-based.
pub struct Args<'a> {
    pub state: &'a mut LuaState,
    name: &'static str,
}

impl Args<'_> {
    pub fn len(&self) -> usize {
        self.state.arg_count()
    }

    /// The argument at `index`, treating nil as absent.
    pub fn get(&self, index: usize) -> Option<LuaValue> {
        self.state.get_arg(index).filter(|v| !v.is_nil())
    }

    pub fn error(&mut self, message: impl Display) -> LuaError {
        self.state.error(message.to_string())
    }

    pub fn arg_error(&mut self, index: usize, message: impl Display) -> LuaError {
        let message = format!("bad argument #{index} to '{}' ({message})", self.name);
        self.state.error(message)
    }

    fn type_error(&mut self, index: usize, expected: &str) -> LuaError {
        let got = self
            .state
            .get_arg(index)
            .map_or("no value", |v| v.type_name());
        self.arg_error(index, format!("{expected} expected, got {got}"))
    }

    pub fn number(&mut self, index: usize) -> LuaResult<f64> {
        match self.state.get_arg(index).and_then(|v| v.as_number()) {
            Some(n) => Ok(n),
            None => Err(self.type_error(index, "number")),
        }
    }

    pub fn opt_number(&mut self, index: usize, default: f64) -> LuaResult<f64> {
        match self.get(index) {
            None => Ok(default),
            Some(_) => self.number(index),
        }
    }

    pub fn f32(&mut self, index: usize) -> LuaResult<f32> {
        self.number(index).map(|n| n as f32)
    }

    pub fn opt_f32(&mut self, index: usize, default: f32) -> LuaResult<f32> {
        self.opt_number(index, default.into()).map(|n| n as f32)
    }

    /// A string argument. Numbers are converted, as with Lua's `tostring`.
    pub fn string(&mut self, index: usize) -> LuaResult<String> {
        match self.state.get_arg(index) {
            Some(v) if v.as_str().is_some() => Ok(v.as_str().unwrap_or_default().to_string()),
            Some(v) if v.as_number().is_some() => Ok(number_to_string(&v)),
            _ => Err(self.type_error(index, "string")),
        }
    }

    pub fn boolean(&self, index: usize) -> bool {
        self.state.get_arg(index).is_some_and(|v| v.is_truthy())
    }

    /// A string argument that must be one of `options`, like Love2D's enums.
    pub fn option<T: Copy>(
        &mut self,
        index: usize,
        what: &str,
        options: &[(&str, T)],
    ) -> LuaResult<T> {
        let name = self.string(index)?;
        if let Some((_, value)) = options.iter().find(|(option, _)| *option == name) {
            return Ok(*value);
        }
        let expected: Vec<String> = options
            .iter()
            .map(|(option, _)| format!("'{option}'"))
            .collect();
        Err(self.arg_error(
            index,
            format!(
                "invalid {what} '{name}', expected one of {}",
                expected.join(", ")
            ),
        ))
    }

    /// Runs `f` on the userdata at `index` if it's a `T`.
    pub fn with_userdata<T: 'static, R>(&self, index: usize, f: impl FnOnce(&T) -> R) -> Option<R> {
        let value = self.state.get_arg(index)?;
        let userdata = value.as_userdata_mut()?;
        userdata.downcast_ref::<T>().map(f)
    }

    /// Like [`Args::with_userdata`], but a type error if the argument isn't a `T`.
    pub fn userdata<T: 'static, R>(
        &mut self,
        index: usize,
        type_name: &str,
        f: impl FnOnce(&T) -> R,
    ) -> LuaResult<R> {
        match self.with_userdata(index, f) {
            Some(result) => Ok(result),
            None => Err(self.type_error(index, type_name)),
        }
    }

    /// Points given either as a table `{x1, y1, x2, y2, ...}` at `start`, or as the numbers from
    /// `start` to the last argument.
    pub fn points(&mut self, start: usize) -> LuaResult<Vec<Vec2>> {
        let mut coords = Vec::new();
        if let Some(table) = self.table(start)? {
            for i in 1.. {
                let value: LuaValue = table.raw_geti(i)?;
                if value.is_nil() {
                    break;
                }
                match value.as_number() {
                    Some(n) => coords.push(n),
                    None => return Err(self.arg_error(start, "table must contain only numbers")),
                }
            }
        } else {
            for i in start..=self.len() {
                coords.push(self.number(i)?);
            }
        }
        if coords.len() % 2 != 0 {
            return Err(self.error(format!(
                "{}: number of vertex components must be a multiple of two",
                self.name
            )));
        }
        Ok(coords
            .chunks_exact(2)
            .map(|c| vec2(c[0] as f32, c[1] as f32))
            .collect())
    }

    /// The table at `index`, if that argument is a table.
    pub fn table(&mut self, index: usize) -> LuaResult<Option<LuaTable>> {
        if self.state.get_arg(index).is_some_and(|v| v.is_table()) {
            self.state.get_arg_as::<LuaTable>(index)
        } else {
            Ok(None)
        }
    }

    /// Pushes return values and reports how many there are.
    pub fn ret(&mut self, values: impl IntoLua) -> LuaResult<usize> {
        self.state.push_multi(values)
    }
}

/// A number to return to Lua. Integral values become Lua integers, so sizes print as `32`
/// rather than `32.0` (Lua 5.5 keeps the two apart; Love2D's LuaJIT doesn't).
pub fn number(n: impl Into<f64>) -> LuaValue {
    let n = n.into();
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        LuaValue::integer(n as i64)
    } else {
        LuaValue::float(n)
    }
}

/// Formats a Lua number the way `tostring` does for common values.
fn number_to_string(value: &LuaValue) -> String {
    if value.is_integer() {
        return value.as_integer().unwrap_or_default().to_string();
    }
    let n = value.as_number().unwrap_or_default();
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e16 {
        format!("{n:.1}")
    } else {
        n.to_string()
    }
}
