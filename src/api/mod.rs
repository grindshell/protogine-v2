//! The Lua-facing `pg` API. Each submodule installs one `pg.*` table; docs/api.md is the spec.
//!
//! Functions are raw luars callbacks wrapped in [`Args`], which gives Love2D-style argument
//! errors (`bad argument #2 to 'rectangle' (number expected, got nil)`) and variadic arguments,
//! neither of which luars' typed callbacks support. [`methods!`] does the same for userdata
//! methods.

/// Defines the Lua methods of a userdata type (one that derives `LuaUserData`) as functions over
/// [`Args`], in place of luars' `#[lua_methods]`. Use it for methods that take variadic
/// arguments or return `self` for chaining. `self` is argument 1, and argument errors count from
/// the argument after it, as in Love2D. It also adds `type()`, which returns the type's name.
///
/// ```ignore
/// methods!(Transform {
///     "translate" => translate,
///     "reset" => |args| { ... },
/// });
/// ```
macro_rules! methods {
    ($type:ident { $($name:literal => $method:expr),* $(,)? }) => {
        impl $type {
            // Shadows luars' blanket `LuaMethodProvider`, which the derived `get_field` calls.
            pub fn __lua_lookup_method(key: &str) -> Option<luars::CFunction> {
                let method: luars::CFunction = match key {
                    $($name => |state| {
                        let method: fn(&mut $crate::api::Args) -> luars::LuaResult<usize> = $method;
                        method(&mut $crate::api::Args::method(state, $name, stringify!($type)))
                    },)*
                    "type" => |state| {
                        state.push(stringify!($type))?;
                        Ok(1)
                    },
                    _ => return None,
                };
                Some(method)
            }
        }
    };
}

mod audio;
mod event;
mod filesystem;
mod graphics;
mod keyboard;
mod math;
mod mouse;
mod system;
mod timer;
mod touch;
mod window;

use std::{cell::RefCell, fmt::Display, rc::Rc};

use luars::{IntoLua, Lua, LuaApi, LuaError, LuaResult, LuaState, LuaTable, LuaValue};
use macroquad::math::{DVec2, Vec2, dvec2};

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
    math::install(lua, pg)?;
    filesystem::install(lua, pg, host)?;
    system::install(lua, pg)?;
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
            .create_closure(move |state: &mut LuaState| {
                f(&mut Args {
                    state,
                    name,
                    self_type: None,
                })
            })?;
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
    /// For a method (see [`methods!`]), the type of `self`, which is argument 1.
    self_type: Option<&'static str>,
}

impl<'a> Args<'a> {
    /// The arguments of a method of `self_type`, called `name`.
    pub fn method(state: &'a mut LuaState, name: &'static str, self_type: &'static str) -> Self {
        Args {
            state,
            name,
            self_type: Some(self_type),
        }
    }

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

    /// An error that names the function, like `polygon: need at least three vertices`.
    pub fn named_error(&mut self, message: impl Display) -> LuaError {
        let message = format!("{}: {message}", self.name);
        self.state.error(message)
    }

    pub fn arg_error(&mut self, index: usize, message: impl Display) -> LuaError {
        // Like Lua's own errors, methods don't count `self`.
        let index = if self.self_type.is_some() {
            index - 1
        } else {
            index
        };
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

    /// An integer argument. Floats with no fractional part count, as in Lua.
    pub fn integer(&mut self, index: usize) -> LuaResult<i64> {
        let value = self.state.get_arg(index);
        match value.as_ref().map(|v| (v.as_integer(), v.as_number())) {
            Some((Some(i), _)) => Ok(i),
            Some((None, Some(_))) => {
                Err(self.arg_error(index, "number has no integer representation"))
            }
            _ => Err(self.type_error(index, "number")),
        }
    }

    pub fn opt_integer(&mut self, index: usize, default: i64) -> LuaResult<i64> {
        match self.get(index) {
            None => Ok(default),
            Some(_) => self.integer(index),
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

    /// A string argument as bytes, which needn't be UTF-8. Numbers are converted, as with
    /// `tostring`.
    pub fn bytes(&mut self, index: usize) -> LuaResult<Vec<u8>> {
        match self.state.get_arg(index) {
            Some(v) if v.is_string() => Ok(v.as_bytes().unwrap_or_default().to_vec()),
            Some(v) if v.as_number().is_some() => Ok(number_to_string(&v).into_bytes()),
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

    /// Runs `f` on `self`, argument 1 of a method made with [`methods!`]. Read the other
    /// arguments first: `f` can't use `self`.
    pub fn this<T: 'static, R>(&mut self, f: impl FnOnce(&mut T) -> R) -> LuaResult<R> {
        let value = self.state.get_arg(1);
        let this = value.as_ref().and_then(|v| v.as_userdata_mut());
        if let Some(this) = this.and_then(|u| u.downcast_mut::<T>()) {
            return Ok(f(this));
        }
        let got = value.map_or("no value", |v| v.type_name());
        let expected = self.self_type.unwrap_or("userdata");
        let message = format!(
            "calling '{}' on bad self ({expected} expected, got {got})",
            self.name
        );
        Err(self.state.error(message))
    }

    /// Returns `self` from a method, for chaining.
    pub fn ret_self(&mut self) -> LuaResult<usize> {
        let this = self.state.get_arg(1).unwrap_or(LuaValue::nil());
        self.state.push_value(this)?;
        Ok(1)
    }

    /// Points given either as a table `{x1, y1, x2, y2, ...}` at `start`, or as the numbers from
    /// `start` to the last argument.
    pub fn points(&mut self, start: usize) -> LuaResult<Vec<Vec2>> {
        Ok(self.vertices(start)?.iter().map(|v| v.as_vec2()).collect())
    }

    /// Like [`Args::points`], in double precision.
    pub fn vertices(&mut self, start: usize) -> LuaResult<Vec<DVec2>> {
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
            return Err(self.named_error("number of vertex components must be a multiple of two"));
        }
        Ok(coords
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[x, y]| dvec2(*x, *y))
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

    /// Pushes a string of any bytes, as another return value.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> LuaResult<()> {
        let value = self.state.create_bytes(bytes)?;
        self.state.push_value(value)
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
