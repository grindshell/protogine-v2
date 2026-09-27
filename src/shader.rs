//! The engine side of Love2D-style shaders.
//!
//! A game writes GLSL ES 1.00 with Love2D's names: an `effect` function for the pixel stage, a
//! `position` function for the vertex stage, or both. This wraps each stage into a complete
//! shader, with a header of declarations and a `main` that calls the game's function.
//!
//! A shader is compiled here once, outside macroquad, to report errors and warnings and to find
//! which uniforms survived compilation. It's then compiled again as a macroquad material for
//! each blend mode it draws with, since a material's blend state is fixed. Uniform values live
//! here, and a material is brought up to date just before it draws. It has no Lua dependency.
//!
//! macroquad renames its attributes and uniforms with a `pg_` prefix (see vendor/README.md), so
//! game code can use any other name.

use std::{cell::RefCell, ffi::CString, rc::Rc, sync::Mutex};

use macroquad::{
    miniquad::{PipelineParams, TextureId, UniformDesc, UniformType, gl},
    prelude::*,
};

use crate::graphics::{Canvas, gl_limit};

/// `GL_MAX_TEXTURE_IMAGE_UNITS`, which miniquad doesn't define.
const GL_MAX_TEXTURE_IMAGE_UNITS: u32 = 0x8872;

/// Texture units macroquad binds before a shader's own images: the drawn texture and the screen.
const RESERVED_TEXTURE_UNITS: i32 = 2;

/// The number of blend-mode materials a shader can have, one per blend mode and alpha mode.
pub const VARIANTS: usize = 12;

/// Used for a stage the game doesn't write.
const DEFAULT_VERTEX: &str = "\
vec4 position(mat4 transform_projection, vec4 vertex_position)
{
    return transform_projection * vertex_position;
}
";

const DEFAULT_PIXEL: &str = "\
vec4 effect(vec4 color, Image tex, vec2 texture_coords, vec2 screen_coords)
{
    return Texel(tex, texture_coords) * color;
}
";

/// Picks `highp` where pixel stages support it. Uniforms that both stages declare must have the
/// same precision in both, so they use this too.
const PRECISION: &str = "\
#ifdef GL_FRAGMENT_PRECISION_HIGH
#define PG_HIGHP highp
#else
#define PG_HIGHP mediump
#endif
";

/// Love2D's names and helpers, for both stages.
const SYNTAX: &str = "\
#define number float
#define Image sampler2D
#define extern uniform

uniform PG_HIGHP mat4 pg_Projection;
uniform PG_HIGHP vec4 love_ScreenSize;

// Drawing applies the transform to VertexPosition, so TransformMatrix is always the identity.
const mat4 TransformMatrix = mat4(1.0);
#define ProjectionMatrix pg_Projection
#define TransformProjectionMatrix pg_Projection

vec4 Texel(sampler2D image, vec2 coords) {
    return texture2D(image, coords);
}

float gammaToLinearPrecise(float c) {
    return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}
vec3 gammaToLinearPrecise(vec3 c) {
    return vec3(gammaToLinearPrecise(c.r), gammaToLinearPrecise(c.g), gammaToLinearPrecise(c.b));
}
vec4 gammaToLinearPrecise(vec4 c) {
    return vec4(gammaToLinearPrecise(c.rgb), c.a);
}
float linearToGammaPrecise(float c) {
    return c < 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}
vec3 linearToGammaPrecise(vec3 c) {
    return vec3(linearToGammaPrecise(c.r), linearToGammaPrecise(c.g), linearToGammaPrecise(c.b));
}
vec4 linearToGammaPrecise(vec4 c) {
    return vec4(linearToGammaPrecise(c.rgb), c.a);
}

// Cheaper approximations, which Love2D uses for gammaToLinear and linearToGamma.
float gammaToLinearFast(float c) {
    return c * (c * (c * 0.305306011 + 0.682171111) + 0.012522878);
}
vec3 gammaToLinearFast(vec3 c) {
    return c * (c * (c * 0.305306011 + 0.682171111) + 0.012522878);
}
vec4 gammaToLinearFast(vec4 c) {
    return vec4(gammaToLinearFast(c.rgb), c.a);
}
float linearToGammaFast(float c) {
    return max(1.055 * pow(max(c, 0.0), 0.41666666) - 0.055, 0.0);
}
vec3 linearToGammaFast(vec3 c) {
    return max(1.055 * pow(max(c, vec3(0.0)), vec3(0.41666666)) - 0.055, vec3(0.0));
}
vec4 linearToGammaFast(vec4 c) {
    return vec4(linearToGammaFast(c.rgb), c.a);
}
#define gammaToLinear gammaToLinearFast
#define linearToGamma linearToGammaFast

// The engine doesn't render gamma-correctly, so these do nothing.
#define gammaCorrectColor
#define unGammaCorrectColor
#define gammaCorrectColorPrecise
#define unGammaCorrectColorPrecise
#define gammaCorrectColorFast
#define unGammaCorrectColorFast
";

const VERTEX_MAIN: &str = "\
uniform highp mat4 pg_Model;

attribute vec3 pg_position;
attribute vec2 pg_texcoord;
attribute vec4 pg_color0;

varying vec4 VaryingTexCoord;
varying vec4 VaryingColor;

vec4 VertexPosition;
vec4 VertexTexCoord;
vec4 VertexColor;
const vec4 ConstantColor = vec4(1.0);

#define love_Position gl_Position

vec4 position(mat4 transform_projection, vec4 vertex_position);

void main() {
    VertexPosition = pg_Model * vec4(pg_position, 1.0);
    VertexTexCoord = vec4(pg_texcoord, 0.0, 1.0);
    VertexColor = pg_color0 / 255.0;
    VaryingTexCoord = VertexTexCoord;
    VaryingColor = VertexColor;
    love_Position = position(TransformProjectionMatrix, VertexPosition);
}
";

const PIXEL_MAIN: &str = "\
vec4 Texel(sampler2D image, vec2 coords, float bias) {
    return texture2D(image, coords, bias);
}

uniform sampler2D pg_Texture;
#define MainTex pg_Texture

varying vec4 VaryingTexCoord;
varying vec4 VaryingColor;

#define love_PixelColor gl_FragColor
// love_ScreenSize.zw flips y on the screen, whose rows go up, but not in a canvas.
#define love_PixelCoord (vec2(gl_FragCoord.x, gl_FragCoord.y * love_ScreenSize.z + love_ScreenSize.w))

vec4 effect(vec4 color, Image tex, vec2 texture_coords, vec2 screen_coords);

void main() {
    love_PixelColor = effect(VaryingColor, MainTex, VaryingTexCoord.st, love_PixelCoord);
}
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Vertex,
    Pixel,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::Vertex => "vertex",
            Stage::Pixel => "pixel",
        }
    }
}

/// A complete shader for `stage` around the game's `code`. `#line 1` makes the compiler count
/// lines from the start of the game's code.
fn wrap(stage: Stage, code: &str) -> String {
    let (define, precision, main) = match stage {
        Stage::Vertex => ("VERTEX", "", VERTEX_MAIN),
        Stage::Pixel => (
            "PIXEL",
            "#ifdef GL_OES_standard_derivatives\n\
             #extension GL_OES_standard_derivatives : enable\n\
             #endif\n\
             precision PG_HIGHP float;\n",
            PIXEL_MAIN,
        ),
    };
    format!("#version 100\n#define {define}\n{PRECISION}{precision}{SYNTAX}{main}#line 1\n{code}")
}

/// What `send` can set a uniform to.
#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    /// `float` or a `vec`, with this many components.
    Float(usize),
    /// `int` or an `ivec`.
    Int(usize),
    /// `bool` or a `bvec`.
    Bool(usize),
    Mat4,
    Image,
    /// A uniform `send` can't set, and why.
    Unsupported(String),
}

impl Kind {
    fn parse(type_name: &str, array: bool) -> Kind {
        let kind = match type_name {
            "float" | "number" => Kind::Float(1),
            "vec2" => Kind::Float(2),
            "vec3" => Kind::Float(3),
            "vec4" => Kind::Float(4),
            "int" => Kind::Int(1),
            "ivec2" => Kind::Int(2),
            "ivec3" => Kind::Int(3),
            "ivec4" => Kind::Int(4),
            "bool" => Kind::Bool(1),
            "bvec2" => Kind::Bool(2),
            "bvec3" => Kind::Bool(3),
            "bvec4" => Kind::Bool(4),
            "mat4" => Kind::Mat4,
            "sampler2D" | "Image" => Kind::Image,
            "mat2" | "mat3" => Kind::Unsupported(format!("{type_name} uniforms aren't supported")),
            "samplerCube" => Kind::Unsupported("cube images aren't supported".into()),
            _ => Kind::Unsupported(format!("uniforms of type '{type_name}' aren't supported")),
        };
        match kind {
            Kind::Image if array => Kind::Unsupported("arrays of images aren't supported".into()),
            kind => kind,
        }
    }

    /// Components per element, for number kinds.
    pub fn components(&self) -> usize {
        match *self {
            Kind::Float(n) | Kind::Int(n) | Kind::Bool(n) => n,
            Kind::Mat4 => 16,
            _ => 0,
        }
    }

    fn uniform_type(&self) -> Option<UniformType> {
        use UniformType::*;
        Some(match *self {
            Kind::Float(1) => Float1,
            Kind::Float(2) => Float2,
            Kind::Float(3) => Float3,
            Kind::Float(4) => Float4,
            Kind::Int(1) | Kind::Bool(1) => Int1,
            Kind::Int(2) | Kind::Bool(2) => Int2,
            Kind::Int(3) | Kind::Bool(3) => Int3,
            Kind::Int(4) | Kind::Bool(4) => Int4,
            Kind::Mat4 => Mat4,
            _ => return None,
        })
    }
}

/// A uniform the shader uses.
pub struct Uniform {
    pub name: String,
    pub kind: Kind,
    /// Elements: 1, or the length of an array, as far as the shader uses it.
    pub count: usize,
}

/// An image a shader can read.
#[derive(Clone)]
pub enum ShaderTexture {
    Image(Texture2D),
    Canvas(Rc<Canvas>),
}

impl ShaderTexture {
    fn texture(&self) -> Texture2D {
        match self {
            ShaderTexture::Image(texture) => texture.clone(),
            ShaderTexture::Canvas(canvas) => canvas.texture().clone(),
        }
    }
}

/// A uniform's current value.
enum Value {
    Floats(Vec<f32>),
    Ints(Vec<i32>),
    /// `None` reads as white.
    Texture(Option<ShaderTexture>),
    /// A kind `send` can't set.
    Unset,
}

/// A shader's material for one blend mode.
struct Variant {
    material: Material,
    /// The shader's `generation` when its values were last copied over.
    synced: Option<u64>,
    screen: Option<[f32; 4]>,
    /// The texture bound to each image uniform, in the order of `Shader::uniforms`.
    textures: Vec<Option<TextureId>>,
}

struct State {
    values: Vec<Value>,
    /// Counts `send`s, so materials know when they're out of date.
    generation: u64,
    variants: [Option<Variant>; VARIANTS],
}

pub struct Shader {
    vertex: String,
    pixel: String,
    uniforms: Vec<Uniform>,
    warnings: String,
    state: RefCell<State>,
}

/// Materials of dropped shaders. macroquad panics if a pending draw call's pipeline is gone, so
/// they're only freed at the start of a frame, when nothing is pending.
static RETIRED: Mutex<Vec<Material>> = Mutex::new(Vec::new());

/// Frees the materials of shaders dropped since the last call. Call it only when no draw calls
/// are pending.
pub fn free_retired() {
    RETIRED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

impl Drop for Shader {
    fn drop(&mut self) {
        let variants = &mut self.state.get_mut().variants;
        RETIRED.lock().unwrap_or_else(|e| e.into_inner()).extend(
            variants
                .iter_mut()
                .filter_map(Option::take)
                .map(|v| v.material),
        );
    }
}

impl Shader {
    /// A shader from one or two strings of code, each holding a pixel stage (a `vec4 effect(`
    /// function), a vertex stage (`vec4 position(`), or both. A stage neither string holds uses
    /// the default.
    pub fn new(first: &str, second: Option<&str>) -> Result<Shader, String> {
        let (mut vertex_code, mut pixel_code) = (None, None);
        for code in std::iter::once(first).chain(second) {
            let code = check_language(&strip_comments(code))?;
            let entry = entry_points(&tokens(&code));
            if entry.multi_canvas {
                return Err(
                    "'void effect()' isn't supported (it draws into several canvases at \
                            once, and only one canvas can be active)"
                        .into(),
                );
            }
            if entry.vertex {
                vertex_code = Some(code.clone());
            }
            if entry.pixel {
                pixel_code = Some(code);
            }
        }
        if vertex_code.is_none() && pixel_code.is_none() {
            return Err(
                "could not parse shader code (missing 'position' or 'effect' function?)".into(),
            );
        }
        let vertex_code = vertex_code.unwrap_or_else(|| DEFAULT_VERTEX.into());
        let pixel_code = pixel_code.unwrap_or_else(|| DEFAULT_PIXEL.into());
        let vertex = wrap(Stage::Vertex, &vertex_code);
        let pixel = wrap(Stage::Pixel, &pixel_code);

        let mut program = Program::link(&vertex, &pixel)?;
        let mut declared = declared_uniforms(&tokens(&vertex_code));
        declared.extend(declared_uniforms(&tokens(&pixel_code)));
        let mut uniforms: Vec<Uniform> = Vec::new();
        for (name, kind, array) in declared {
            if uniforms.iter().any(|u| u.name == name) || !program.has_uniform(&name) {
                continue;
            }
            let count = if array { program.array_len(&name) } else { 1 };
            uniforms.push(Uniform { name, kind, count });
        }

        let images = uniforms.iter().filter(|u| u.kind == Kind::Image).count() as i32;
        let units = gl_limit(GL_MAX_TEXTURE_IMAGE_UNITS);
        if units > 0 && RESERVED_TEXTURE_UNITS + images > units {
            return Err(format!(
                "the shader uses {images} Image uniforms, more than this GPU's limit of {}",
                units - RESERVED_TEXTURE_UNITS
            ));
        }

        let values = uniforms
            .iter()
            .map(|u| match u.kind {
                Kind::Float(_) | Kind::Mat4 => {
                    Value::Floats(vec![0.0; u.count * u.kind.components()])
                }
                Kind::Int(_) | Kind::Bool(_) => Value::Ints(vec![0; u.count * u.kind.components()]),
                Kind::Image => Value::Texture(None),
                Kind::Unsupported(_) => Value::Unset,
            })
            .collect();
        Ok(Shader {
            vertex,
            pixel,
            uniforms,
            warnings: std::mem::take(&mut program.warnings),
            state: RefCell::new(State {
                values,
                generation: 0,
                variants: Default::default(),
            }),
        })
    }

    /// The compiler's warnings, by stage; often empty.
    pub fn warnings(&self) -> &str {
        &self.warnings
    }

    /// A uniform the shader uses, and its index.
    pub fn uniform(&self, name: &str) -> Option<(usize, &Uniform)> {
        self.uniforms
            .iter()
            .enumerate()
            .find(|(_, u)| u.name == name)
    }

    /// Sets the start of a float, vec or mat4 uniform's elements; the rest keep their values.
    /// Matrices are column-major.
    pub fn set_floats(&self, index: usize, values: &[f32]) {
        let mut state = self.state.borrow_mut();
        if let Value::Floats(current) = &mut state.values[index] {
            let n = values.len().min(current.len());
            current[..n].copy_from_slice(&values[..n]);
        }
        state.generation += 1;
    }

    /// Like [`Shader::set_floats`], for int, ivec, bool and bvec uniforms.
    pub fn set_ints(&self, index: usize, values: &[i32]) {
        let mut state = self.state.borrow_mut();
        if let Value::Ints(current) = &mut state.values[index] {
            let n = values.len().min(current.len());
            current[..n].copy_from_slice(&values[..n]);
        }
        state.generation += 1;
    }

    pub fn set_texture(&self, index: usize, texture: ShaderTexture) {
        let mut state = self.state.borrow_mut();
        state.values[index] = Value::Texture(Some(texture));
        state.generation += 1;
    }

    /// The material to draw with under blend mode `variant` (see [`VARIANTS`]), created by
    /// `params` on first use, with the shader's current values. `screen` is `love_ScreenSize`
    /// for the target, and `target` the active canvas, if any.
    pub fn prepare(
        &self,
        variant: usize,
        params: impl FnOnce() -> PipelineParams,
        screen: [f32; 4],
        target: Option<&Rc<Canvas>>,
    ) -> Result<Material, String> {
        let mut state = self.state.borrow_mut();
        let State {
            values,
            generation,
            variants,
        } = &mut *state;

        if let Some(target) = target {
            for (uniform, value) in self.uniforms.iter().zip(values.iter()) {
                if let Value::Texture(Some(ShaderTexture::Canvas(canvas))) = value
                    && Rc::ptr_eq(canvas, target)
                {
                    return Err(format!(
                        "cannot draw into a Canvas that the active shader reads (it was sent to \
                         '{}')",
                        uniform.name
                    ));
                }
            }
        }

        let slot = &mut variants[variant];
        let fresh = slot.is_none();
        if fresh {
            *slot = Some(self.new_variant(params())?);
        }
        let Some(variant) = slot else {
            unreachable!("the variant was just created")
        };
        let material = &variant.material;

        if variant.synced != Some(*generation) {
            let mut images = variant.textures.iter_mut();
            for (uniform, value) in self.uniforms.iter().zip(values.iter()) {
                match value {
                    Value::Floats(floats) => material.set_uniform_array(&uniform.name, floats),
                    Value::Ints(ints) => material.set_uniform_array(&uniform.name, ints),
                    Value::Texture(texture) => {
                        let Some(bound) = images.next() else { continue };
                        let texture = texture
                            .as_ref()
                            .map_or_else(Texture2D::empty, ShaderTexture::texture);
                        let id = texture.raw_miniquad_id();
                        if *bound != Some(id) {
                            // macroquad reads a material's textures when it runs the batch,
                            // not per draw call, so earlier draws must run first.
                            if !fresh {
                                // SAFETY: the borrow ends here, and nothing else touches
                                // macroquad meanwhile.
                                unsafe { get_internal_gl() }.flush();
                            }
                            material.set_texture(&uniform.name, texture);
                            *bound = Some(id);
                        }
                    }
                    Value::Unset => {}
                }
            }
            variant.synced = Some(*generation);
        }

        if variant.screen != Some(screen) {
            material.set_uniform("love_ScreenSize", screen);
            variant.screen = Some(screen);
        }
        Ok(material.clone())
    }

    fn new_variant(&self, pipeline_params: PipelineParams) -> Result<Variant, String> {
        let mut uniforms = vec![UniformDesc::new("love_ScreenSize", UniformType::Float4)];
        let mut textures = Vec::new();
        for uniform in &self.uniforms {
            if let Some(uniform_type) = uniform.kind.uniform_type() {
                uniforms.push(UniformDesc::new(&uniform.name, uniform_type).array(uniform.count));
            } else if uniform.kind == Kind::Image {
                textures.push(uniform.name.clone());
            }
        }
        let shader = ShaderSource::Glsl {
            vertex: &self.vertex,
            fragment: &self.pixel,
        };
        let params = MaterialParams {
            pipeline_params,
            uniforms,
            textures,
        };
        let material = load_material(shader, params).map_err(|e| match e {
            macroquad::Error::UnknownError(_) => {
                "too many shaders are in use (create shaders once, rather than every frame)"
                    .to_string()
            }
            e => format!("could not create the shader: {e}"),
        })?;
        let images = self
            .uniforms
            .iter()
            .filter(|u| u.kind == Kind::Image)
            .count();
        Ok(Variant {
            material,
            synced: None,
            screen: None,
            textures: vec![None; images],
        })
    }
}

/// A program compiled and linked outside macroquad, to check the code and look up uniforms.
struct Program {
    id: u32,
    warnings: String,
}

impl Drop for Program {
    fn drop(&mut self) {
        // SAFETY: `id` is a program this module created and nothing else uses.
        unsafe { gl::glDeleteProgram(self.id) };
    }
}

/// A compiled stage, deleted once linked or abandoned.
struct StageObject(u32);

impl Drop for StageObject {
    fn drop(&mut self) {
        // SAFETY: the shader object is this module's own. If it's attached to a program, GL
        // deletes it along with the program.
        unsafe { gl::glDeleteShader(self.0) };
    }
}

impl Program {
    fn link(vertex: &str, pixel: &str) -> Result<Program, String> {
        let (vertex, vertex_log) = compile(Stage::Vertex, vertex)?;
        let (pixel, pixel_log) = compile(Stage::Pixel, pixel)?;
        // SAFETY: the GL context exists while a game runs. This only creates, links and queries
        // objects of its own, and binds nothing.
        let (mut program, linked, program_log) = unsafe {
            let program = Program {
                id: gl::glCreateProgram(),
                warnings: String::new(),
            };
            gl::glAttachShader(program.id, vertex.0);
            gl::glAttachShader(program.id, pixel.0);
            gl::glLinkProgram(program.id);
            let mut linked = 0;
            gl::glGetProgramiv(program.id, gl::GL_LINK_STATUS, &mut linked);
            let mut length = 0;
            gl::glGetProgramiv(program.id, gl::GL_INFO_LOG_LENGTH, &mut length);
            let log = info_log(length, |size, buffer| {
                gl::glGetProgramInfoLog(program.id, size, std::ptr::null_mut(), buffer)
            });
            (program, linked != 0, log)
        };
        if !linked {
            return Err(format!("could not link shader code:\n{program_log}"));
        }
        for (what, log) in [
            ("vertex shader", vertex_log),
            ("pixel shader", pixel_log),
            ("program", program_log),
        ] {
            if !log.is_empty() {
                program.warnings.push_str(&format!("{what}:\n{log}\n"));
            }
        }
        Ok(program)
    }

    fn location(&self, name: &str) -> i32 {
        let Ok(name) = CString::new(name) else {
            return -1;
        };
        // SAFETY: a query on this module's own program.
        unsafe { gl::glGetUniformLocation(self.id, name.as_ptr() as *const _) }
    }

    /// Whether compiling kept the uniform: compilers remove uniforms the code never reads.
    fn has_uniform(&self, name: &str) -> bool {
        self.location(name) != -1
    }

    /// How many elements of the array uniform `name` compiling kept.
    fn array_len(&self, name: &str) -> usize {
        (1..4096)
            .find(|i| self.location(&format!("{name}[{i}]")) == -1)
            .unwrap_or(4096)
    }
}

/// Compiles one stage, returning it and its warnings, or an error with the compiler's messages.
fn compile(stage: Stage, source: &str) -> Result<(StageObject, String), String> {
    let kind = match stage {
        Stage::Vertex => gl::GL_VERTEX_SHADER,
        Stage::Pixel => gl::GL_FRAGMENT_SHADER,
    };
    let source =
        CString::new(source).map_err(|_| "shader code can't contain NUL characters".to_string())?;
    // SAFETY: as in `Program::link`.
    let (object, compiled, log) = unsafe {
        let object = StageObject(gl::glCreateShader(kind));
        let text = source.as_ptr() as *const _;
        gl::glShaderSource(object.0, 1, &text, std::ptr::null());
        gl::glCompileShader(object.0);
        let mut compiled = 0;
        gl::glGetShaderiv(object.0, gl::GL_COMPILE_STATUS, &mut compiled);
        let mut length = 0;
        gl::glGetShaderiv(object.0, gl::GL_INFO_LOG_LENGTH, &mut length);
        let log = info_log(length, |size, buffer| {
            gl::glGetShaderInfoLog(object.0, size, std::ptr::null_mut(), buffer)
        });
        (object, compiled != 0, log)
    };
    if !compiled {
        return Err(format!(
            "could not compile {} shader code:\n{log}",
            stage.name()
        ));
    }
    Ok((object, log))
}

/// Reads an info log `length` bytes long (including its NUL) with `read`.
fn info_log(length: i32, read: impl FnOnce(i32, *mut gl::GLchar)) -> String {
    // WebGL reports 1 for an empty log.
    if length <= 1 {
        return String::new();
    }
    let mut buffer = vec![0u8; length as usize];
    read(length, buffer.as_mut_ptr() as *mut _);
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end])
        .trim_end()
        .to_string()
}

/// Replaces comments with spaces, keeping line breaks so line numbers stay put. That also keeps
/// strict compilers from rejecting characters in comments that GLSL ES doesn't allow.
fn strip_comments(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                while chars.next_if(|&next| next != '\n').is_some() {}
                out.push(' ');
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    if next == '\n' {
                        out.push('\n');
                    }
                    previous = next;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Blanks out `#pragma language glsl1`, and rejects any other language.
fn check_language(code: &str) -> Result<String, String> {
    let mut out = String::with_capacity(code.len());
    for line in code.split_inclusive('\n') {
        let directive = line.trim_start().strip_prefix('#').unwrap_or_default();
        let mut words = directive.split_whitespace();
        if (words.next(), words.next()) == (Some("pragma"), Some("language")) {
            match words.next() {
                Some("glsl1") => {
                    if line.ends_with('\n') {
                        out.push('\n');
                    }
                    continue;
                }
                language => {
                    return Err(format!(
                        "unsupported shader language '{}' (shaders are GLSL ES 1.00, Love2D's \
                         'glsl1', because the web build uses WebGL 1)",
                        language.unwrap_or_default()
                    ));
                }
            }
        }
        out.push_str(line);
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Token<'a> {
    Word(&'a str),
    Punct(char),
}

/// The words and punctuation of comment-free code, leaving out preprocessor lines.
fn tokens(code: &str) -> Vec<Token<'_>> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut tokens = Vec::new();
    for line in code.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let mut rest = line;
        while let Some(c) = rest.chars().next() {
            if is_word(c) {
                let end = rest.find(|c: char| !is_word(c)).unwrap_or(rest.len());
                tokens.push(Token::Word(&rest[..end]));
                rest = &rest[end..];
            } else {
                if !c.is_whitespace() {
                    tokens.push(Token::Punct(c));
                }
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    tokens
}

/// Which of Love2D's entry points some code defines.
#[derive(Default, PartialEq, Debug)]
struct EntryPoints {
    vertex: bool,
    pixel: bool,
    /// `void effect()`, which draws into several canvases.
    multi_canvas: bool,
}

fn entry_points(tokens: &[Token]) -> EntryPoints {
    let mut found = EntryPoints::default();
    for window in tokens.windows(3) {
        match window {
            [
                Token::Word("vec4"),
                Token::Word("position"),
                Token::Punct('('),
            ] => found.vertex = true,
            [
                Token::Word("vec4"),
                Token::Word("effect"),
                Token::Punct('('),
            ] => found.pixel = true,
            [
                Token::Word("void"),
                Token::Word("effect"),
                Token::Punct('('),
            ] => found.multi_canvas = true,
            _ => {}
        }
    }
    found
}

/// The uniforms some code declares, with `uniform` or `extern`: their names, kinds, and whether
/// they're arrays.
fn declared_uniforms(tokens: &[Token]) -> Vec<(String, Kind, bool)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let keyword = matches!(tokens[i], Token::Word("uniform" | "extern"));
        i += 1;
        if !keyword {
            continue;
        }
        while matches!(
            tokens.get(i),
            Some(Token::Word("lowp" | "mediump" | "highp"))
        ) {
            i += 1;
        }
        let Some(Token::Word(type_name)) = tokens.get(i) else {
            continue;
        };
        i += 1;
        // One or more names, like `uniform float a, b[4];`.
        while let Some(Token::Word(name)) = tokens.get(i) {
            i += 1;
            let array = tokens.get(i) == Some(&Token::Punct('['));
            if array {
                while let Some(token) = tokens.get(i) {
                    i += 1;
                    if *token == Token::Punct(']') {
                        break;
                    }
                }
            }
            found.push((name.to_string(), Kind::parse(type_name, array), array));
            if tokens.get(i) != Some(&Token::Punct(',')) {
                break;
            }
            i += 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_keeping_lines() {
        let code = "a // one\nb /* two\nthree */ c\n/*/ d */e";
        assert_eq!(strip_comments(code), "a  \nb \n  c\n e");
    }

    #[test]
    fn finds_entry_points() {
        let find = |code: &str| entry_points(&tokens(&strip_comments(code)));
        let pixel = find("vec4 effect (vec4 c, Image t, vec2 uv, vec2 sc) { return c; }");
        assert!(pixel.pixel && !pixel.vertex);
        let both = find("vec4 position(mat4 m, vec4 p) {}\nvec4\neffect(vec4 c) {}");
        assert!(both.pixel && both.vertex);
        assert!(find("void effect() {}").multi_canvas);
        // Comments, preprocessor lines and calls don't count.
        let none = find("// vec4 effect(\n#define X vec4 effect(\nvec4 x = position(m, p);");
        assert_eq!(none, EntryPoints::default());
    }

    #[test]
    fn checks_the_language() {
        assert_eq!(
            check_language("#pragma language glsl1\nx\n").unwrap(),
            "\nx\n"
        );
        assert_eq!(check_language("  # pragma language glsl1").unwrap(), "");
        let error = check_language("#pragma language glsl3\n").unwrap_err();
        assert!(
            error.starts_with("unsupported shader language 'glsl3'"),
            "{error}"
        );
        assert_eq!(
            check_language("#pragma optimize(on)\n").unwrap(),
            "#pragma optimize(on)\n"
        );
    }

    #[test]
    fn finds_declared_uniforms() {
        let code = "extern number time;\nuniform highp vec2 a, b[4];\nextern Image tex;\n\
                    uniform mat3 m;\nuniform sampler2D many[2];\nvec4 uniformish;";
        let names: Vec<(String, Kind, bool)> = declared_uniforms(&tokens(code));
        let expected = [
            ("time", Kind::Float(1), false),
            ("a", Kind::Float(2), false),
            ("b", Kind::Float(2), true),
            ("tex", Kind::Image, false),
            (
                "m",
                Kind::Unsupported("mat3 uniforms aren't supported".into()),
                false,
            ),
            (
                "many",
                Kind::Unsupported("arrays of images aren't supported".into()),
                true,
            ),
        ];
        assert_eq!(names.len(), expected.len());
        for ((name, kind, array), (want_name, want_kind, want_array)) in names.iter().zip(expected)
        {
            assert_eq!(
                (name.as_str(), kind, *array),
                (want_name, &want_kind, want_array)
            );
        }
    }

    #[test]
    fn wraps_code_after_a_line_directive() {
        let pixel = wrap(Stage::Pixel, "code");
        assert!(pixel.starts_with("#version 100\n#define PIXEL\n"));
        assert!(pixel.ends_with("#line 1\ncode"));
        assert!(wrap(Stage::Vertex, "code").contains("#define VERTEX\n"));
    }
}
