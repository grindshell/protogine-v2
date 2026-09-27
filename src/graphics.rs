//! Love2D-style immediate-mode 2D graphics on top of macroquad.
//!
//! This is the engine side of `pg.graphics`; `api::graphics` maps Lua arguments onto it.
//! Every draw call runs under the current transform, pushed onto macroquad's model-matrix stack
//! just for that call, and with the material for the current blend mode: the blend mode's own,
//! or the active shader's (see `shader`).
//!
//! macroquad batches draw calls and only runs them when the camera changes or the frame ends.
//! Switching the target (the screen or a canvas) switches the camera, so every pending draw
//! call belongs to the current target.

use std::{cell::Cell, rc::Rc, sync::OnceLock};

use macroquad::{
    miniquad::{BlendFactor, BlendState, BlendValue, Equation, PassAction, gl},
    models::Vertex,
    prelude::*,
    text::Font as MqFont,
};

use crate::shader::{self, Shader};

/// Love2D caps the transform stack at 64 entries.
pub const MAX_STACK_DEPTH: usize = 64;
pub const DEFAULT_FONT_SIZE: u16 = 16;

/// `GL_MAX_SAMPLES`, which miniquad doesn't define.
const GL_MAX_SAMPLES: u32 = 0x8D57;

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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    Alpha,
    Add,
    Subtract,
    Multiply,
    Screen,
    Replace,
}

/// Whether drawn colors still need multiplying by their alpha (`Multiply`, Love2D's
/// `"alphamultiply"`) or already are (`Premultiplied`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    Multiply,
    Premultiplied,
}

/// The blend state for one blend mode and alpha mode, as Love2D 11 sets it up.
fn blend_params(mode: BlendMode, alpha: AlphaMode) -> PipelineParams {
    use BlendFactor::{One, OneMinusValue, Value, Zero};
    use BlendValue::{DestinationColor, SourceAlpha, SourceColor};
    let (equation, src_rgb, src_alpha, dst_rgb, dst_alpha) = match mode {
        BlendMode::Alpha => (
            Equation::Add,
            One,
            One,
            OneMinusValue(SourceAlpha),
            OneMinusValue(SourceAlpha),
        ),
        BlendMode::Add => (Equation::Add, One, Zero, One, One),
        BlendMode::Subtract => (Equation::ReverseSubtract, One, Zero, One, One),
        BlendMode::Multiply => (
            Equation::Add,
            Value(DestinationColor),
            Value(DestinationColor),
            Zero,
            Zero,
        ),
        BlendMode::Screen => (
            Equation::Add,
            One,
            One,
            OneMinusValue(SourceColor),
            OneMinusValue(SourceColor),
        ),
        BlendMode::Replace => (Equation::Add, One, One, Zero, Zero),
    };
    // Colors that aren't premultiplied get multiplied by their alpha on the way in.
    let src_rgb = match (src_rgb, alpha) {
        (One, AlphaMode::Multiply) => Value(SourceAlpha),
        (factor, _) => factor,
    };
    PipelineParams {
        color_blend: Some(BlendState::new(equation, src_rgb, dst_rgb)),
        alpha_blend: Some(BlendState::new(equation, src_alpha, dst_alpha)),
        ..Default::default()
    }
}

/// macroquad's default shader, with a medium-precision `uv` so large textures sample correctly
/// on phones.
const VERTEX_SHADER: &str = r#"#version 100
attribute vec3 pg_position;
attribute vec2 pg_texcoord;
attribute vec4 pg_color0;

varying lowp vec4 color;
varying mediump vec2 uv;

uniform mat4 pg_Model;
uniform mat4 pg_Projection;

void main() {
    gl_Position = pg_Projection * pg_Model * vec4(pg_position, 1);
    color = pg_color0 / 255.0;
    uv = pg_texcoord;
}"#;

const FRAGMENT_SHADER: &str = r#"#version 100
varying lowp vec4 color;
varying mediump vec2 uv;

uniform sampler2D pg_Texture;

void main() {
    gl_FragColor = color * texture2D(pg_Texture, uv);
}"#;

/// The material that draws with a blend mode. macroquad's default material blends alpha like
/// color, which leaves translucent drawing in a canvas too transparent, so every draw uses one
/// of these.
///
/// Each is created on first use and never freed: macroquad panics if a pending draw call's
/// pipeline is gone, and a game that stops mid-frame leaves draw calls pending.
fn blend_material(mode: BlendMode, alpha: AlphaMode) -> &'static Material {
    static MATERIALS: [OnceLock<Material>; shader::VARIANTS] =
        [const { OnceLock::new() }; shader::VARIANTS];
    MATERIALS[blend_index(mode, alpha)].get_or_init(|| {
        let shader = ShaderSource::Glsl {
            vertex: VERTEX_SHADER,
            fragment: FRAGMENT_SHADER,
        };
        let params = MaterialParams {
            pipeline_params: blend_params(mode, alpha),
            ..Default::default()
        };
        load_material(shader, params).expect("the blend shader is valid GLSL 100")
    })
}

/// A number for each blend mode and alpha mode, below [`shader::VARIANTS`].
fn blend_index(mode: BlendMode, alpha: AlphaMode) -> usize {
    mode as usize * 2 + alpha as usize
}

/// Reads a GL limit such as `GL_MAX_TEXTURE_SIZE`.
pub(crate) fn gl_limit(name: u32) -> i32 {
    let mut value = 0;
    // SAFETY: the window, and so the GL context, exists whenever `Graphics` does, and this only
    // reads state.
    unsafe { gl::glGetIntegerv(name, &mut value) };
    value
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

/// An offscreen target: `width` by `height` units, with `dpi_scale` pixels per unit.
pub struct Canvas {
    target: RenderTarget,
    width: f32,
    height: f32,
    dpi_scale: f32,
    /// MSAA samples, 1 for none.
    samples: i32,
    filter: Cell<FilterMode>,
}

impl Canvas {
    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn pixel_width(&self) -> f32 {
        self.target.texture.width()
    }

    pub fn pixel_height(&self) -> f32 {
        self.target.texture.height()
    }

    pub fn dpi_scale(&self) -> f32 {
        self.dpi_scale
    }

    /// MSAA samples, or 0 without MSAA, as Love2D reports it.
    pub fn msaa(&self) -> i32 {
        if self.samples > 1 { self.samples } else { 0 }
    }

    pub fn filter(&self) -> FilterMode {
        self.filter.get()
    }

    pub fn set_filter(&self, filter: FilterMode) {
        self.target.texture.set_filter(filter);
        self.filter.set(filter);
    }

    pub(crate) fn texture(&self) -> &Texture2D {
        &self.target.texture
    }
}

/// Drawing into a canvas: its units map onto its pixels, with y pointing down.
impl Camera for Canvas {
    fn matrix(&self) -> Mat4 {
        // GL puts a texture's first row at the bottom of clip space, and drawing a texture puts
        // its first row at the top, so y = 0 goes to the bottom here to come out on top.
        Mat4::orthographic_rh_gl(0.0, self.width, 0.0, self.height, -1.0, 1.0)
    }

    fn depth_enabled(&self) -> bool {
        false
    }

    fn render_pass(&self) -> Option<RenderPass> {
        Some(self.target.render_pass.clone())
    }

    fn viewport(&self) -> Option<(i32, i32, i32, i32)> {
        None
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
    blend: (BlendMode, AlphaMode),
    /// The active canvas, or `None` for the screen.
    canvas: Option<Rc<Canvas>>,
    /// The active shader, or `None` for the default.
    shader: Option<Rc<Shader>>,
    transform: Mat4,
    stack: Vec<Mat4>,
    /// The largest texture side the GPU supports, or 0 if it didn't say.
    max_texture_size: i32,
    /// The most MSAA samples a canvas can have; 1 where it can't have any.
    max_samples: i32,
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
            blend: (BlendMode::Alpha, AlphaMode::Multiply),
            canvas: None,
            shader: None,
            transform: Mat4::IDENTITY,
            stack: Vec::new(),
            max_texture_size: gl_limit(gl::GL_MAX_TEXTURE_SIZE),
            // WebGL 1 has no multisampled render targets.
            max_samples: if cfg!(target_arch = "wasm32") {
                1
            } else {
                gl_limit(GL_MAX_SAMPLES).max(1)
            },
        }
    }

    /// Clears the screen to the background color and resets the transform stack, before
    /// `pg.draw`. The screen is cleared even if the game left a canvas active.
    pub fn begin_frame(&mut self) {
        if self.canvas.is_some() {
            set_default_camera();
        }
        clear_background(self.background);
        if let Some(canvas) = &self.canvas {
            set_camera(canvas.as_ref());
        }
        // Switching cameras ran the canvas's pending draws, and clearing dropped the screen's.
        shader::free_retired();
        self.transform = Mat4::IDENTITY;
        self.stack.clear();
    }

    /// Hands macroquad back to the engine's screens: drawing to the screen, with its default
    /// material.
    pub fn shutdown(&mut self) {
        self.canvas = None;
        self.shader = None;
        set_default_camera();
        gl_use_default_material();
    }

    /// Clears the active target. With no color, the screen clears to the background color and
    /// a canvas to transparent black.
    pub fn clear(&self, color: Option<Color>) {
        let fallback = if self.canvas.is_some() {
            Color::new(0.0, 0.0, 0.0, 0.0)
        } else {
            self.background
        };
        clear_background(color.unwrap_or(fallback));
    }

    // ---- blending ----

    pub fn blend_mode(&self) -> (BlendMode, AlphaMode) {
        self.blend
    }

    pub fn set_blend_mode(&mut self, mode: BlendMode, alpha: AlphaMode) -> Result<(), String> {
        if mode == BlendMode::Multiply && alpha == AlphaMode::Multiply {
            return Err("the 'multiply' blend mode must be used with premultiplied alpha".into());
        }
        self.blend = (mode, alpha);
        Ok(())
    }

    // ---- canvases ----

    /// A canvas cleared to transparent black. `msaa` is the number of samples asked for; 0 or 1
    /// means none.
    pub fn new_canvas(
        &self,
        width: f32,
        height: f32,
        dpi_scale: f32,
        msaa: i32,
    ) -> Result<Canvas, String> {
        let pixels = |units: f32| (units * dpi_scale).round().max(1.0);
        let (pixel_width, pixel_height) = (pixels(width), pixels(height));
        let max = self.max_texture_size as f32;
        if max > 0.0 && (pixel_width > max || pixel_height > max) {
            return Err(format!(
                "{pixel_width}x{pixel_height} pixels is larger than this GPU's limit of {max} \
                 pixels on a side"
            ));
        }
        let samples = if msaa > 1 {
            msaa.min(self.max_samples)
        } else {
            1
        };
        let target = render_target_ex(
            pixel_width as u32,
            pixel_height as u32,
            RenderTargetParams {
                sample_count: samples,
                depth: false,
            },
        );
        target.texture.set_filter(self.default_filter);

        // New render textures hold whatever was in that memory natively.
        // SAFETY: the borrow ends within this block and nothing else touches macroquad meanwhile.
        let gl = unsafe { get_internal_gl() }.quad_context;
        gl.begin_pass(
            Some(target.render_pass.raw_miniquad_id()),
            PassAction::clear_color(0.0, 0.0, 0.0, 0.0),
        );
        gl.end_render_pass();

        Ok(Canvas {
            target,
            width,
            height,
            dpi_scale,
            samples,
            filter: Cell::new(self.default_filter),
        })
    }

    pub fn canvas(&self) -> Option<Rc<Canvas>> {
        self.canvas.clone()
    }

    /// Makes `canvas`, or the screen for `None`, the target of drawing. Returns the previous
    /// canvas.
    pub fn set_canvas(&mut self, canvas: Option<Rc<Canvas>>) -> Option<Rc<Canvas>> {
        match &canvas {
            Some(canvas) => set_camera(canvas.as_ref()),
            None => set_default_camera(),
        }
        std::mem::replace(&mut self.canvas, canvas)
    }

    // ---- shaders ----

    pub fn shader(&self) -> Option<Rc<Shader>> {
        self.shader.clone()
    }

    /// Draws with `shader`, or with the default for `None`.
    pub fn set_shader(&mut self, shader: Option<Rc<Shader>>) {
        self.shader = shader;
    }

    /// `love_ScreenSize` for the active target: its size in pixels, and how to flip
    /// `gl_FragCoord.y` so that y points down. Screen rows go up; a canvas's rows already point
    /// down (see `Camera for Canvas`).
    fn screen_size(&self) -> [f32; 4] {
        match &self.canvas {
            Some(canvas) => [canvas.pixel_width(), canvas.pixel_height(), 1.0, 0.0],
            None => {
                let (width, height) = macroquad::miniquad::window::screen_size();
                [width, height, -1.0, height]
            }
        }
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
    //
    // Drawing fails only if the active shader can't draw: see `Graphics::use_material`.

    pub fn rectangle(&self, mode: ShapeMode, x: f32, y: f32, w: f32, h: f32) -> Result<(), String> {
        let points = [
            vec2(x, y),
            vec2(x + w, y),
            vec2(x + w, y + h),
            vec2(x, y + h),
        ];
        self.shape(mode, &points, true)
    }

    pub fn ellipse(&self, mode: ShapeMode, x: f32, y: f32, rx: f32, ry: f32) -> Result<(), String> {
        let segments =
            ((((rx.abs() + ry.abs()) / 2.0) * 20.0).sqrt().ceil() as usize).clamp(8, 256);
        let points: Vec<Vec2> = (0..segments)
            .map(|i| {
                let angle = i as f32 / segments as f32 * std::f32::consts::TAU;
                vec2(x + rx * angle.cos(), y + ry * angle.sin())
            })
            .collect();
        self.shape(mode, &points, true)
    }

    pub fn polygon(&self, mode: ShapeMode, points: &[Vec2]) -> Result<(), String> {
        self.shape(mode, points, true)
    }

    pub fn line(&self, points: &[Vec2]) -> Result<(), String> {
        self.shape(ShapeMode::Line, points, false)
    }

    pub fn points(&self, points: &[Vec2]) -> Result<(), String> {
        let size = self.point_size;
        self.with_transform(self.transform, || {
            for p in points {
                draw_rectangle(p.x - size / 2.0, p.y - size / 2.0, size, size, self.color);
            }
        })
    }

    fn shape(&self, mode: ShapeMode, points: &[Vec2], closed: bool) -> Result<(), String> {
        self.with_transform(self.transform, || match mode {
            ShapeMode::Fill => fill_convex(points, self.color),
            ShapeMode::Line => stroke(points, closed, self.line_width, self.color),
        })
    }

    // ---- images and text ----

    /// Draws an image, or the part of it under `quad`, with `local` (a placement or a
    /// Transform's matrix) on top of the current transform.
    pub fn draw_image(
        &self,
        texture: &Texture2D,
        quad: Option<Rect>,
        local: Mat4,
    ) -> Result<(), String> {
        let size = quad.map_or(texture.size(), |q| q.size());
        self.draw_texture(texture, quad, size, local)
    }

    /// Like [`Graphics::draw_image`], for a canvas. `quad` is in the canvas's units.
    pub fn draw_canvas(
        &self,
        canvas: &Rc<Canvas>,
        quad: Option<Rect>,
        local: Mat4,
    ) -> Result<(), String> {
        if self
            .canvas
            .as_ref()
            .is_some_and(|active| Rc::ptr_eq(active, canvas))
        {
            return Err("cannot draw a Canvas into itself".into());
        }
        let s = canvas.dpi_scale;
        let source = quad.map(|q| Rect::new(q.x * s, q.y * s, q.w * s, q.h * s));
        let size = quad.map_or(vec2(canvas.width, canvas.height), |q| q.size());
        self.draw_texture(&canvas.target.texture, source, size, local)
    }

    /// Draws `source` (in pixels; all of `texture` for `None`) stretched to `size` units.
    fn draw_texture(
        &self,
        texture: &Texture2D,
        source: Option<Rect>,
        size: Vec2,
        local: Mat4,
    ) -> Result<(), String> {
        self.with_transform(self.transform * local, || {
            draw_texture_ex(
                texture,
                0.0,
                0.0,
                self.color,
                DrawTextureParams {
                    source,
                    dest_size: Some(size),
                    ..Default::default()
                },
            );
        })
    }

    pub fn print(&self, text: &str, local: Mat4) -> Result<(), String> {
        let font = &self.font;
        self.with_transform(self.transform * local, || {
            for (i, line) in text.split('\n').enumerate() {
                font.draw_line(line, 0.0, i, self.color);
            }
        })
    }

    pub fn printf(&self, text: &str, limit: f32, align: Align, local: Mat4) -> Result<(), String> {
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
        })
    }

    /// Runs `draw` with `matrix` as macroquad's model matrix, and the current material.
    fn with_transform(&self, matrix: Mat4, draw: impl FnOnce()) -> Result<(), String> {
        self.use_material()?;
        // SAFETY: each borrow of the GL context ends before `draw` touches macroquad again.
        unsafe { get_internal_gl() }
            .quad_gl
            .push_model_matrix(matrix);
        draw();
        unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
        Ok(())
    }

    /// Switches to the material for the blend mode: the active shader's, or else the blend
    /// mode's own. Set on every draw, since macroquad's own UI pass resets the material each
    /// frame. Fails if the shader reads the active canvas, or can't make another material.
    fn use_material(&self) -> Result<(), String> {
        let (mode, alpha) = self.blend;
        match &self.shader {
            None => gl_use_material(blend_material(mode, alpha)),
            Some(shader) => {
                let material = shader.prepare(
                    blend_index(mode, alpha),
                    || blend_params(mode, alpha),
                    self.screen_size(),
                    self.canvas.as_ref(),
                )?;
                gl_use_material(&material);
            }
        }
        Ok(())
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
