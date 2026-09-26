//! Love2D-style immediate-mode 2D graphics on top of macroquad.
//!
//! This is the engine side of `pg.graphics`; `api::graphics` maps Lua arguments onto it.
//! Every draw call runs under the current transform, pushed onto macroquad's model-matrix stack
//! just for that call.

use std::{cell::Cell, rc::Rc};

use macroquad::{models::Vertex, prelude::*, text::Font as MqFont};

/// Love2D caps the transform stack at 64 entries.
pub const MAX_STACK_DEPTH: usize = 64;
pub const DEFAULT_FONT_SIZE: u16 = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShapeMode {
    Fill,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Position, rotation, scale, origin offset and shear, as taken by `pg.graphics.draw`, `print`
/// and `printf`, and by `pg.math.newTransform`.
#[derive(Clone, Copy)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub sx: f32,
    pub sy: f32,
    pub ox: f32,
    pub oy: f32,
    pub kx: f32,
    pub ky: f32,
}

impl Placement {
    /// Translate, rotate, scale, shear, then offset by the origin, multiplied out as Love2D does.
    pub fn matrix(&self) -> Mat4 {
        let Placement {
            x,
            y,
            r,
            sx,
            sy,
            ox,
            oy,
            kx,
            ky,
        } = *self;
        let (s, c) = r.sin_cos();
        let (a, b) = (c * sx - ky * s * sy, s * sx + ky * c * sy);
        let (cc, d) = (kx * c * sx - s * sy, kx * s * sx + c * sy);
        Mat4::from_cols(
            vec4(a, b, 0.0, 0.0),
            vec4(cc, d, 0.0, 0.0),
            Vec4::Z,
            vec4(x - ox * a - oy * cc, y - ox * b - oy * d, 0.0, 1.0),
        )
    }
}

/// Shears x by `kx` times y, and y by `ky` times x.
pub fn shear_matrix(kx: f32, ky: f32) -> Mat4 {
    Mat4::from_cols(
        vec4(1.0, ky, 0.0, 0.0),
        vec4(kx, 1.0, 0.0, 0.0),
        Vec4::Z,
        Vec4::W,
    )
}

pub struct Image {
    pub texture: Texture2D,
    filter: Cell<FilterMode>,
}

impl Image {
    pub fn from_bytes(bytes: &[u8], filter: FilterMode) -> Result<Image, String> {
        let image = macroquad::texture::Image::from_file_with_format(bytes, None)
            .map_err(|e| e.to_string())?;
        let texture = Texture2D::from_image(&image);
        texture.set_filter(filter);
        Ok(Image {
            texture,
            filter: Cell::new(filter),
        })
    }

    pub fn width(&self) -> f32 {
        self.texture.width()
    }

    pub fn height(&self) -> f32 {
        self.texture.height()
    }

    pub fn filter(&self) -> FilterMode {
        self.filter.get()
    }

    pub fn set_filter(&self, filter: FilterMode) {
        self.texture.set_filter(filter);
        self.filter.set(filter);
    }
}

pub struct Font {
    /// `None` is macroquad's built-in font.
    font: Option<MqFont>,
    size: u16,
    /// Distance from the top of a line to its baseline.
    ascent: f32,
    height: f32,
    filter: Cell<FilterMode>,
}

impl Font {
    pub fn builtin(size: u16, filter: FilterMode) -> Font {
        Font::new(None, size, filter)
    }

    pub fn from_ttf(bytes: &[u8], size: u16, filter: FilterMode) -> Result<Font, String> {
        let font = macroquad::text::load_ttf_font_from_bytes(bytes).map_err(|e| e.to_string())?;
        Ok(Font::new(Some(font), size, filter))
    }

    fn new(font: Option<MqFont>, size: u16, filter: FilterMode) -> Font {
        // macroquad doesn't expose line metrics, so measure glyphs that span the full
        // ascent and descent in most fonts. Glyph extents undershoot the line spacing of
        // small-glyph fonts like the built-in one, so lines are at least `size` tall.
        let metrics = measure_text("Mg|", font.as_ref(), size, 1.0);
        let font = Font {
            font,
            size,
            ascent: metrics.offset_y,
            height: metrics.height.max(f32::from(size)),
            filter: Cell::new(filter),
        };
        font.set_filter(filter);
        font
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn width(&self, text: &str) -> f32 {
        text.split('\n')
            .map(|line| measure_text(line, self.font.as_ref(), self.size, 1.0).width)
            .fold(0.0, f32::max)
    }

    pub fn filter(&self) -> FilterMode {
        self.filter.get()
    }

    /// Note: the built-in font's glyph atlas is shared, so filtering one built-in `Font`
    /// filters them all.
    pub fn set_filter(&self, filter: FilterMode) {
        match &self.font {
            Some(font) => font.clone().set_filter(filter),
            None => macroquad::text::get_default_font().set_filter(filter),
        }
        self.filter.set(filter);
    }

    fn draw_line(&self, text: &str, x: f32, line: usize, color: Color) {
        draw_text_ex(
            text,
            x,
            self.ascent + line as f32 * self.height,
            TextParams {
                font: self.font.as_ref(),
                font_size: self.size,
                color,
                ..Default::default()
            },
        );
    }
}

pub struct Graphics {
    pub color: Color,
    pub background: Color,
    pub line_width: f32,
    pub point_size: f32,
    pub font: Rc<Font>,
    pub default_filter: FilterMode,
    transform: Mat4,
    stack: Vec<Mat4>,
}

impl Graphics {
    /// Needs macroquad's context, so call it once the window exists.
    pub fn new() -> Graphics {
        Graphics {
            color: WHITE,
            background: BLACK,
            line_width: 1.0,
            point_size: 1.0,
            font: Rc::new(Font::builtin(DEFAULT_FONT_SIZE, FilterMode::Linear)),
            default_filter: FilterMode::Linear,
            transform: Mat4::IDENTITY,
            stack: Vec::new(),
        }
    }

    /// Clears to the background color and resets the transform stack, before `pg.draw`.
    pub fn begin_frame(&mut self) {
        clear_background(self.background);
        self.transform = Mat4::IDENTITY;
        self.stack.clear();
    }

    pub fn clear(&self, color: Color) {
        clear_background(color);
    }

    // ---- transforms ----

    pub fn push(&mut self) -> Result<(), &'static str> {
        if self.stack.len() >= MAX_STACK_DEPTH {
            return Err("maximum stack depth reached (more pushes than pops?)");
        }
        self.stack.push(self.transform);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<(), &'static str> {
        self.transform = self
            .stack
            .pop()
            .ok_or("minimum stack depth reached (more pops than pushes?)")?;
        Ok(())
    }

    pub fn origin(&mut self) {
        self.transform = Mat4::IDENTITY;
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.transform *= Mat4::from_translation(vec3(dx, dy, 0.0));
    }

    pub fn rotate(&mut self, angle: f32) {
        self.transform *= Mat4::from_rotation_z(angle);
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.transform *= Mat4::from_scale(vec3(sx, sy, 1.0));
    }

    pub fn shear(&mut self, kx: f32, ky: f32) {
        self.transform *= shear_matrix(kx, ky);
    }

    pub fn apply_transform(&mut self, matrix: Mat4) {
        self.transform *= matrix;
    }

    pub fn replace_transform(&mut self, matrix: Mat4) {
        self.transform = matrix;
    }

    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        let p = self.transform.transform_point3(vec3(x, y, 0.0));
        (p.x, p.y)
    }

    pub fn inverse_transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        let p = self.transform.inverse().transform_point3(vec3(x, y, 0.0));
        (p.x, p.y)
    }

    // ---- shapes ----

    pub fn rectangle(&self, mode: ShapeMode, x: f32, y: f32, w: f32, h: f32) {
        let points = [
            vec2(x, y),
            vec2(x + w, y),
            vec2(x + w, y + h),
            vec2(x, y + h),
        ];
        self.shape(mode, &points, true);
    }

    pub fn ellipse(&self, mode: ShapeMode, x: f32, y: f32, rx: f32, ry: f32) {
        let segments =
            ((((rx.abs() + ry.abs()) / 2.0) * 20.0).sqrt().ceil() as usize).clamp(8, 256);
        let points: Vec<Vec2> = (0..segments)
            .map(|i| {
                let angle = i as f32 / segments as f32 * std::f32::consts::TAU;
                vec2(x + rx * angle.cos(), y + ry * angle.sin())
            })
            .collect();
        self.shape(mode, &points, true);
    }

    pub fn polygon(&self, mode: ShapeMode, points: &[Vec2]) {
        self.shape(mode, points, true);
    }

    pub fn line(&self, points: &[Vec2]) {
        self.shape(ShapeMode::Line, points, false);
    }

    pub fn points(&self, points: &[Vec2]) {
        let size = self.point_size;
        self.with_transform(self.transform, || {
            for p in points {
                draw_rectangle(p.x - size / 2.0, p.y - size / 2.0, size, size, self.color);
            }
        });
    }

    fn shape(&self, mode: ShapeMode, points: &[Vec2], closed: bool) {
        self.with_transform(self.transform, || match mode {
            ShapeMode::Fill => fill_convex(points, self.color),
            ShapeMode::Line => stroke(points, closed, self.line_width, self.color),
        });
    }

    // ---- images and text ----

    /// Draws with `local` (a placement or a Transform's matrix) on top of the current transform.
    pub fn draw_texture(&self, texture: &Texture2D, source: Option<Rect>, local: Mat4) {
        self.with_transform(self.transform * local, || {
            draw_texture_ex(
                texture,
                0.0,
                0.0,
                self.color,
                DrawTextureParams {
                    source,
                    ..Default::default()
                },
            );
        });
    }

    pub fn print(&self, text: &str, local: Mat4) {
        let font = &self.font;
        self.with_transform(self.transform * local, || {
            for (i, line) in text.split('\n').enumerate() {
                font.draw_line(line, 0.0, i, self.color);
            }
        });
    }

    pub fn printf(&self, text: &str, limit: f32, align: Align, local: Mat4) {
        let font = &self.font;
        let wrapped = wrap_text(text, font.font.as_ref(), font.size, 1.0, limit.max(1.0));
        self.with_transform(self.transform * local, || {
            for (i, line) in wrapped.split('\n').enumerate() {
                let x = match align {
                    Align::Left => 0.0,
                    Align::Center => (limit - font.width(line)) / 2.0,
                    Align::Right => limit - font.width(line),
                };
                font.draw_line(line, x, i, self.color);
            }
        });
    }

    /// Runs `draw` with `matrix` as macroquad's model matrix.
    fn with_transform(&self, matrix: Mat4, draw: impl FnOnce()) {
        // SAFETY: each borrow of the GL context ends before `draw` touches macroquad again.
        unsafe { get_internal_gl() }
            .quad_gl
            .push_model_matrix(matrix);
        draw();
        unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
    }
}

/// Fills a convex polygon as a triangle fan.
fn fill_convex(points: &[Vec2], color: Color) {
    if points.len() < 3 {
        return;
    }
    let vertices: Vec<Vertex> = points
        .iter()
        .map(|p| Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, color))
        .collect();
    let indices: Vec<u16> = (1..points.len() as u16 - 1)
        .flat_map(|i| [0, i, i + 1])
        .collect();

    // SAFETY: the borrow ends within this function and nothing else touches macroquad meanwhile.
    let gl = unsafe { get_internal_gl() }.quad_gl;
    gl.texture(None);
    gl.draw_mode(DrawMode::Triangles);
    gl.geometry(&vertices, &indices);
}

fn stroke(points: &[Vec2], closed: bool, width: f32, color: Color) {
    for pair in points.windows(2) {
        draw_line(pair[0].x, pair[0].y, pair[1].x, pair[1].y, width, color);
    }
    if closed && points.len() > 2 {
        let (first, last) = (points[0], points[points.len() - 1]);
        draw_line(last.x, last.y, first.x, first.y, width, color);
    }
}
