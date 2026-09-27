//! `pg.graphics`: drawing state, shapes, images, canvases, text and transforms.

use std::rc::Rc;

use luars::{Lua, LuaResult, LuaTable, LuaUserData, LuaValue, lua_methods};
use macroquad::prelude::{
    Color, FilterMode, Mat4, Rect, Texture2D, screen_dpi_scale, screen_height, screen_width,
};

use super::{Args, Module, SharedHost, math::Transform, number};
use crate::graphics::{
    self as gfx, Align, AlphaMode, BlendMode, DEFAULT_FONT_SIZE, Graphics, Placement, ShapeMode,
};

const SHAPE_MODES: &[(&str, ShapeMode)] = &[("fill", ShapeMode::Fill), ("line", ShapeMode::Line)];
const FILTERS: &[(&str, FilterMode)] = &[
    ("linear", FilterMode::Linear),
    ("nearest", FilterMode::Nearest),
];
const ALIGNS: &[(&str, Align)] = &[
    ("left", Align::Left),
    ("center", Align::Center),
    ("right", Align::Right),
];
const BLEND_MODES: &[(&str, BlendMode)] = &[
    ("alpha", BlendMode::Alpha),
    ("add", BlendMode::Add),
    ("subtract", BlendMode::Subtract),
    ("multiply", BlendMode::Multiply),
    ("screen", BlendMode::Screen),
    ("replace", BlendMode::Replace),
];
const ALPHA_MODES: &[(&str, AlphaMode)] = &[
    ("alphamultiply", AlphaMode::Multiply),
    ("premultiplied", AlphaMode::Premultiplied),
];

/// The name of an enum value, from its table of options.
fn option_name<T: PartialEq>(options: &[(&'static str, T)], value: T) -> &'static str {
    options
        .iter()
        .find(|(_, option)| *option == value)
        .map_or("", |(name, _)| name)
}

fn filter_name(filter: FilterMode) -> String {
    match filter {
        FilterMode::Linear => "linear",
        FilterMode::Nearest => "nearest",
    }
    .to_string()
}

fn parse_filter(name: &str) -> Result<FilterMode, String> {
    FILTERS
        .iter()
        .find(|(option, _)| *option == name)
        .map(|(_, filter)| *filter)
        .ok_or_else(|| format!("invalid filter mode '{name}', expected one of 'linear', 'nearest'"))
}

#[derive(LuaUserData)]
pub struct Image {
    image: gfx::Image,
}

#[lua_methods]
impl Image {
    #[lua(name = "getWidth")]
    pub fn get_width(&self) -> LuaValue {
        number(self.image.width())
    }

    #[lua(name = "getHeight")]
    pub fn get_height(&self) -> LuaValue {
        number(self.image.height())
    }

    #[lua(name = "getDimensions")]
    pub fn get_dimensions(&self) -> (LuaValue, LuaValue) {
        (number(self.image.width()), number(self.image.height()))
    }

    #[lua(name = "setFilter")]
    pub fn set_filter(&self, filter: String) -> Result<(), String> {
        self.image.set_filter(parse_filter(&filter)?);
        Ok(())
    }

    #[lua(name = "getFilter")]
    pub fn get_filter(&self) -> String {
        filter_name(self.image.filter())
    }

    #[lua(name = "type")]
    pub fn lua_type(&self) -> String {
        "Image".to_string()
    }
}

#[derive(LuaUserData)]
pub struct Quad {
    rect: Rect,
}

#[lua_methods]
impl Quad {
    #[lua(name = "getViewport")]
    pub fn get_viewport(&self) -> (LuaValue, LuaValue, LuaValue, LuaValue) {
        let r = self.rect;
        (number(r.x), number(r.y), number(r.w), number(r.h))
    }

    #[lua(name = "setViewport")]
    pub fn set_viewport(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.rect = Rect::new(x as f32, y as f32, w as f32, h as f32);
    }

    #[lua(name = "type")]
    pub fn lua_type(&self) -> String {
        "Quad".to_string()
    }
}

#[derive(LuaUserData)]
#[lua_impl(PartialEq)]
pub struct Canvas {
    canvas: Rc<gfx::Canvas>,
    /// For `renderTo`, which switches the target.
    host: SharedHost,
}

/// `getCanvas` returns new userdata for the active canvas, so equality compares the underlying
/// canvas rather than the userdata.
impl PartialEq for Canvas {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.canvas, &other.canvas)
    }
}

/// Runs `f` on the canvas that's `self` in a method call.
fn canvas<R>(args: &mut Args, f: impl FnOnce(&gfx::Canvas) -> R) -> LuaResult<R> {
    args.this(|c: &mut Canvas| f(&c.canvas))
}

methods!(Canvas {
    "getWidth" => |args| {
        let width = canvas(args, |c| c.width())?;
        args.ret(number(width))
    },
    "getHeight" => |args| {
        let height = canvas(args, |c| c.height())?;
        args.ret(number(height))
    },
    "getDimensions" => |args| {
        let (width, height) = canvas(args, |c| (c.width(), c.height()))?;
        args.ret((number(width), number(height)))
    },
    "getPixelWidth" => |args| {
        let width = canvas(args, |c| c.pixel_width())?;
        args.ret(number(width))
    },
    "getPixelHeight" => |args| {
        let height = canvas(args, |c| c.pixel_height())?;
        args.ret(number(height))
    },
    "getPixelDimensions" => |args| {
        let (width, height) = canvas(args, |c| (c.pixel_width(), c.pixel_height()))?;
        args.ret((number(width), number(height)))
    },
    "getDPIScale" => |args| {
        let scale = canvas(args, |c| c.dpi_scale())?;
        args.ret(number(scale))
    },
    "getMSAA" => |args| {
        let msaa = canvas(args, |c| c.msaa())?;
        args.ret(i64::from(msaa))
    },
    "setFilter" => |args| {
        let filter = args.option(2, "filter mode", FILTERS)?;
        canvas(args, |c| c.set_filter(filter))?;
        Ok(0)
    },
    "getFilter" => |args| {
        let filter = canvas(args, |c| c.filter())?;
        args.ret(filter_name(filter))
    },
    "renderTo" => render_to,
});

/// `canvas:renderTo(func, ...)`: calls `func(...)` with the canvas active, then restores the
/// previous target, even if `func` raises an error.
fn render_to(args: &mut Args) -> LuaResult<usize> {
    let func = match args.get(2) {
        Some(func) if func.is_function() => func,
        _ => return Err(args.type_error(2, "function")),
    };
    let rest: Vec<LuaValue> = (3..=args.len())
        .map(|i| args.state.get_arg(i).unwrap_or(LuaValue::nil()))
        .collect();
    let (target, host) = args.this(|c: &mut Canvas| (c.canvas.clone(), c.host.clone()))?;

    let previous = host.borrow_mut().graphics().set_canvas(Some(target));
    let result = args.state.call(func, rest);
    host.borrow_mut().graphics().set_canvas(previous);
    result.map(|_| 0)
}

/// Something `pg.graphics.draw` can draw.
enum Drawable {
    Image(Texture2D),
    Canvas(Rc<gfx::Canvas>),
}

#[derive(LuaUserData)]
pub struct Font {
    font: Rc<gfx::Font>,
}

#[lua_methods]
impl Font {
    #[lua(name = "getWidth")]
    pub fn get_width(&self, text: String) -> LuaValue {
        number(self.font.width(&text))
    }

    #[lua(name = "getHeight")]
    pub fn get_height(&self) -> LuaValue {
        number(self.font.height())
    }

    #[lua(name = "setFilter")]
    pub fn set_filter(&self, filter: String) -> Result<(), String> {
        self.font.set_filter(parse_filter(&filter)?);
        Ok(())
    }

    #[lua(name = "getFilter")]
    pub fn get_filter(&self) -> String {
        filter_name(self.font.filter())
    }

    #[lua(name = "type")]
    pub fn lua_type(&self) -> String {
        "Font".to_string()
    }
}

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    let mut m = Module::new(lua)?;

    // Registers `pg.graphics.<name>`, handing the closure the args and the graphics state.
    macro_rules! function {
        ($name:literal, |$args:ident, $g:ident| $body:expr) => {{
            let host = host.clone();
            m.function($name, move |$args: &mut Args| {
                let mut host = host.borrow_mut();
                let $g: &mut Graphics = host.graphics();
                $body
            })?;
        }};
    }

    // ---- state ----

    function!("setColor", |args, g| {
        g.color = color(args, 1)?;
        Ok(0)
    });
    function!("getColor", |args, g| args.ret(color_values(g.color)));
    function!("setBackgroundColor", |args, g| {
        g.background = color(args, 1)?;
        Ok(0)
    });
    function!("getBackgroundColor", |args, g| args
        .ret(color_values(g.background)));
    function!("setLineWidth", |args, g| {
        g.line_width = args.f32(1)?;
        Ok(0)
    });
    function!("getLineWidth", |args, g| args.ret(number(g.line_width)));
    function!("setPointSize", |args, g| {
        g.point_size = args.f32(1)?;
        Ok(0)
    });
    function!("getPointSize", |args, g| args.ret(number(g.point_size)));
    function!("setFont", |args, g| {
        g.font = args.userdata(1, "Font", |font: &Font| font.font.clone())?;
        Ok(0)
    });
    function!("getFont", |args, g| {
        let font = Font {
            font: g.font.clone(),
        };
        args.state.push(font)?;
        Ok(1)
    });
    function!("setDefaultFilter", |args, g| {
        g.default_filter = args.option(1, "filter mode", FILTERS)?;
        Ok(0)
    });
    function!("getDefaultFilter", |args, g| args
        .ret(filter_name(g.default_filter)));
    function!("getWidth", |args, _g| args.ret(number(screen_width())));
    function!("getHeight", |args, _g| args.ret(number(screen_height())));
    function!("getDimensions", |args, _g| {
        args.ret((number(screen_width()), number(screen_height())))
    });
    function!("clear", |args, g| {
        let c = if args.get(1).is_some() {
            Some(color(args, 1)?)
        } else {
            None
        };
        g.clear(c);
        Ok(0)
    });
    function!("setBlendMode", |args, g| {
        let mode = args.option(1, "blend mode", BLEND_MODES)?;
        let alpha = if args.get(2).is_some() {
            args.option(2, "blend alpha mode", ALPHA_MODES)?
        } else {
            AlphaMode::Multiply
        };
        g.set_blend_mode(mode, alpha).map_err(|e| args.error(e))?;
        Ok(0)
    });
    function!("getBlendMode", |args, g| {
        let (mode, alpha) = g.blend_mode();
        args.ret((
            option_name(BLEND_MODES, mode),
            option_name(ALPHA_MODES, alpha),
        ))
    });

    // ---- shapes ----

    function!("rectangle", |args, g| {
        let mode = args.option(1, "draw mode", SHAPE_MODES)?;
        let (x, y, w, h) = (args.f32(2)?, args.f32(3)?, args.f32(4)?, args.f32(5)?);
        g.rectangle(mode, x, y, w, h);
        Ok(0)
    });
    function!("circle", |args, g| {
        let mode = args.option(1, "draw mode", SHAPE_MODES)?;
        let (x, y, radius) = (args.f32(2)?, args.f32(3)?, args.f32(4)?);
        g.ellipse(mode, x, y, radius, radius);
        Ok(0)
    });
    function!("ellipse", |args, g| {
        let mode = args.option(1, "draw mode", SHAPE_MODES)?;
        let (x, y, rx, ry) = (args.f32(2)?, args.f32(3)?, args.f32(4)?, args.f32(5)?);
        g.ellipse(mode, x, y, rx, ry);
        Ok(0)
    });
    function!("polygon", |args, g| {
        let mode = args.option(1, "draw mode", SHAPE_MODES)?;
        let points = args.points(2)?;
        if points.len() < 3 {
            return Err(args.error("polygon: need at least three vertices"));
        }
        g.polygon(mode, &points);
        Ok(0)
    });
    function!("line", |args, g| {
        let points = args.points(1)?;
        if points.len() < 2 {
            return Err(args.error("line: need at least two points"));
        }
        g.line(&points);
        Ok(0)
    });
    function!("points", |args, g| {
        let points = args.points(1)?;
        g.points(&points);
        Ok(0)
    });

    // ---- images ----

    {
        let host = host.clone();
        m.function("newImage", move |args| {
            let path = args.string(1)?;
            let result = {
                let mut host = host.borrow_mut();
                let filter = host.graphics().default_filter;
                host.fs
                    .read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| gfx::Image::from_bytes(&bytes, filter))
            };
            match result {
                Ok(image) => {
                    args.state.push(Image { image })?;
                    Ok(1)
                }
                Err(e) => Err(args.error(format!("could not load image '{path}': {e}"))),
            }
        })?;
    }
    m.function("newQuad", |args| {
        let rect = Rect::new(args.f32(1)?, args.f32(2)?, args.f32(3)?, args.f32(4)?);
        args.state.push(Quad { rect })?;
        Ok(1)
    })?;
    function!("draw", |args, g| {
        let drawable =
            if let Some(texture) = args.with_userdata(1, |i: &Image| i.image.texture.clone()) {
                Drawable::Image(texture)
            } else if let Some(canvas) = args.with_userdata(1, |c: &Canvas| c.canvas.clone()) {
                Drawable::Canvas(canvas)
            } else {
                return Err(args.type_error(1, "Drawable"));
            };
        let (quad, first) = match args.with_userdata(2, |quad: &Quad| quad.rect) {
            Some(rect) => (Some(rect), 3),
            None => (None, 2),
        };
        let local = local_transform(args, first)?;
        match drawable {
            Drawable::Image(texture) => g.draw_image(&texture, quad, local),
            Drawable::Canvas(canvas) => g
                .draw_canvas(&canvas, quad, local)
                .map_err(|e| args.error(e))?,
        }
        Ok(0)
    });

    // ---- canvases ----

    {
        let host = host.clone();
        m.function("newCanvas", move |args| {
            let width = canvas_size(args, 1, screen_width())?;
            let height = canvas_size(args, 2, screen_height())?;
            let (msaa, dpi_scale) = canvas_settings(args, 3)?;
            let result = host
                .borrow_mut()
                .graphics()
                .new_canvas(width, height, dpi_scale, msaa);
            match result {
                Ok(canvas) => {
                    args.state.push(Canvas {
                        canvas: Rc::new(canvas),
                        host: host.clone(),
                    })?;
                    Ok(1)
                }
                Err(e) => Err(args.error(format!("could not create canvas: {e}"))),
            }
        })?;
    }
    function!("setCanvas", |args, g| {
        let canvas = match args.get(1) {
            None => None,
            Some(_) => Some(args.userdata(1, "Canvas", |c: &Canvas| c.canvas.clone())?),
        };
        g.set_canvas(canvas);
        Ok(0)
    });
    {
        let host = host.clone();
        m.function("getCanvas", move |args| {
            let active = host.borrow_mut().graphics().canvas();
            match active {
                Some(canvas) => {
                    args.state.push(Canvas {
                        canvas,
                        host: host.clone(),
                    })?;
                    Ok(1)
                }
                None => args.ret(LuaValue::nil()),
            }
        })?;
    }

    // ---- text ----

    {
        let host = host.clone();
        m.function("newFont", move |args| {
            let result = if args.get(1).is_some_and(|v| v.as_number().is_some()) {
                let size = font_size(args, 1)?;
                let filter = host.borrow_mut().graphics().default_filter;
                Ok(gfx::Font::builtin(size, filter))
            } else {
                let path = args.string(1)?;
                let size = if args.get(2).is_some() {
                    font_size(args, 2)?
                } else {
                    DEFAULT_FONT_SIZE
                };
                let mut host = host.borrow_mut();
                let filter = host.graphics().default_filter;
                host.fs
                    .read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| gfx::Font::from_ttf(&bytes, size, filter))
                    .map_err(|e| format!("could not load font '{path}': {e}"))
            };
            match result {
                Ok(font) => {
                    args.state.push(Font {
                        font: Rc::new(font),
                    })?;
                    Ok(1)
                }
                Err(e) => Err(args.error(e)),
            }
        })?;
    }
    function!("print", |args, g| {
        let text = args.string(1)?;
        let local = local_transform(args, 2)?;
        g.print(&text, local);
        Ok(0)
    });
    function!("printf", |args, g| {
        let text = args.string(1)?;
        // Either `x, y, limit, align, r, sx, ...` or `transform, limit, align`.
        let given = transform(args, 2);
        let limit_at = if given.is_some() { 3 } else { 4 };
        let limit = args.f32(limit_at)?;
        let align = if args.get(limit_at + 1).is_some() {
            args.option(limit_at + 1, "align mode", ALIGNS)?
        } else {
            Align::Left
        };
        let local = match given {
            Some(matrix) => matrix,
            None => {
                let (x, y) = (args.f32(2)?, args.f32(3)?);
                placement_at(args, x, y, 6)?.matrix()
            }
        };
        g.printf(&text, limit, align, local);
        Ok(0)
    });

    // ---- transforms ----

    function!("push", |args, g| g
        .push()
        .map(|()| 0)
        .map_err(|e| args.error(e)));
    function!("pop", |args, g| g
        .pop()
        .map(|()| 0)
        .map_err(|e| args.error(e)));
    function!("origin", |_args, g| {
        g.origin();
        Ok(0)
    });
    function!("translate", |args, g| {
        g.translate(args.f32(1)?, args.f32(2)?);
        Ok(0)
    });
    function!("rotate", |args, g| {
        g.rotate(args.f32(1)?);
        Ok(0)
    });
    function!("scale", |args, g| {
        let sx = args.f32(1)?;
        let sy = args.opt_f32(2, sx)?;
        g.scale(sx, sy);
        Ok(0)
    });
    function!("shear", |args, g| {
        g.shear(args.f32(1)?, args.f32(2)?);
        Ok(0)
    });
    function!("applyTransform", |args, g| {
        let matrix = args.userdata(1, "Transform", |t: &Transform| t.matrix)?;
        g.apply_transform(matrix);
        Ok(0)
    });
    function!("replaceTransform", |args, g| {
        let matrix = args.userdata(1, "Transform", |t: &Transform| t.matrix)?;
        g.replace_transform(matrix);
        Ok(0)
    });
    function!("transformPoint", |args, g| {
        let (x, y) = g.transform_point(args.f32(1)?, args.f32(2)?);
        args.ret((number(x), number(y)))
    });
    function!("inverseTransformPoint", |args, g| {
        let (x, y) = g.inverse_transform_point(args.f32(1)?, args.f32(2)?);
        args.ret((number(x), number(y)))
    });

    m.finish(pg, "graphics")
}

/// A color given as `r, g, b, a` or `{r, g, b, a}` starting at `index`; alpha defaults to 1.
fn color(args: &mut Args, index: usize) -> LuaResult<Color> {
    if let Some(table) = args.table(index)? {
        let mut c = [1.0f32; 4];
        for (i, component) in c.iter_mut().enumerate() {
            let value: Option<f64> = table
                .raw_geti(i as i64 + 1)
                .map_err(|_| args.arg_error(index, "color components must be numbers"))?;
            match value {
                Some(v) => *component = v as f32,
                None if i == 3 => {}
                None => return Err(args.arg_error(index, "color table needs at least 3 numbers")),
            }
        }
        return Ok(Color::new(c[0], c[1], c[2], c[3]));
    }
    Ok(Color::new(
        args.f32(index)?,
        args.f32(index + 1)?,
        args.f32(index + 2)?,
        args.opt_f32(index + 3, 1.0)?,
    ))
}

fn color_values(c: Color) -> (LuaValue, LuaValue, LuaValue, LuaValue) {
    (number(c.r), number(c.g), number(c.b), number(c.a))
}

/// `x, y, r, sx, sy, ox, oy, kx, ky` starting at `start`, with Love2D's defaults.
pub fn placement(args: &mut Args, start: usize) -> LuaResult<Placement> {
    let x = args.opt_f32(start, 0.0)?;
    let y = args.opt_f32(start + 1, 0.0)?;
    placement_at(args, x, y, start + 2)
}

/// A placement at `x, y`, with `r, sx, sy, ox, oy, kx, ky` starting at `start`.
fn placement_at(args: &mut Args, x: f32, y: f32, start: usize) -> LuaResult<Placement> {
    let r = args.opt_f32(start, 0.0)?;
    let sx = args.opt_f32(start + 1, 1.0)?;
    let sy = args.opt_f32(start + 2, sx)?;
    let ox = args.opt_f32(start + 3, 0.0)?;
    let oy = args.opt_f32(start + 4, 0.0)?;
    let kx = args.opt_f32(start + 5, 0.0)?;
    let ky = args.opt_f32(start + 6, 0.0)?;
    Ok(Placement {
        x,
        y,
        r,
        sx,
        sy,
        ox,
        oy,
        kx,
        ky,
    })
}

/// Where to draw one object: a Transform at `start`, or a placement starting there.
fn local_transform(args: &mut Args, start: usize) -> LuaResult<Mat4> {
    match transform(args, start) {
        Some(matrix) => Ok(matrix),
        None => Ok(placement(args, start)?.matrix()),
    }
}

/// The matrix of the Transform at `index`, if that argument is one.
fn transform(args: &Args, index: usize) -> Option<Mat4> {
    args.with_userdata(index, |t: &Transform| t.matrix)
}

/// A canvas width or height at `index`: a positive whole number, or `default` rounded.
fn canvas_size(args: &mut Args, index: usize, default: f32) -> LuaResult<f32> {
    let size = args.opt_integer(index, default.round() as i64)?;
    if size < 1 {
        let what = if index == 1 { "width" } else { "height" };
        return Err(args.arg_error(index, format!("{what} must be positive")));
    }
    Ok(size as f32)
}

/// `newCanvas`'s optional settings table at `index`: `msaa` and `dpiscale`.
fn canvas_settings(args: &mut Args, index: usize) -> LuaResult<(i32, f32)> {
    let (mut msaa, mut dpi_scale) = (0, screen_dpi_scale());
    let Some(settings) = args.table(index)? else {
        if args.get(index).is_some() {
            return Err(args.type_error(index, "table"));
        }
        return Ok((msaa, dpi_scale));
    };
    for (key, value) in settings.pairs_raw()? {
        match key.as_str() {
            Some("msaa") => match value.as_number() {
                Some(n) if n.fract() == 0.0 && (0.0..=64.0).contains(&n) => msaa = n as i32,
                _ => return Err(args.arg_error(index, "msaa must be a whole number from 0 to 64")),
            },
            Some("dpiscale") => match value.as_number() {
                Some(n) if n.is_finite() && n > 0.0 => dpi_scale = n as f32,
                _ => return Err(args.arg_error(index, "dpiscale must be a positive number")),
            },
            _ => {
                let name = key.as_str().map_or_else(|| key.to_string(), str::to_string);
                return Err(args.arg_error(
                    index,
                    format!("invalid canvas setting '{name}', expected one of 'msaa', 'dpiscale'"),
                ));
            }
        }
    }
    Ok((msaa, dpi_scale))
}

fn font_size(args: &mut Args, index: usize) -> LuaResult<u16> {
    let size = args.number(index)?;
    if !(1.0..=1024.0).contains(&size) {
        return Err(args.arg_error(index, "font size must be between 1 and 1024"));
    }
    Ok(size as u16)
}
