//! `pg.math`: random numbers, noise, color conversion, polygons, Bézier curves and transforms.

use std::{cell::RefCell, rc::Rc};

use luars::{Lua, LuaApi, LuaResult, LuaTable, LuaUserData, LuaValue};
use macroquad::{
    math::{DVec2, Mat4, dvec2, vec3},
    miniquad::date,
};

use super::{Args, Module, graphics::placement, number};
use crate::{
    graphics::shear_matrix,
    math::{self as engine, Rng},
};

pub fn install(lua: &mut Lua, pg: &LuaTable) -> LuaResult<()> {
    let mut m = Module::new(lua)?;

    // pg.math's own generator, seeded from the clock like Love2D's.
    let rng = Rc::new(RefCell::new(Rng::new((date::now() * 1000.0) as u64)));
    let random_functions: [(&str, RandomFunction); 6] = [
        ("random", random),
        ("randomNormal", random_normal),
        ("setRandomSeed", set_seed),
        ("getRandomSeed", get_seed),
        ("setRandomState", set_state),
        ("getRandomState", get_state),
    ];
    for (name, f) in random_functions {
        let rng = rng.clone();
        m.function(name, move |args| f(args, Generator::Global(&rng)))?;
    }
    m.function("newRandomGenerator", |args| {
        let rng = match args.get(1) {
            None => Rng::default(),
            Some(_) => Rng::new(seed(args, 1)?),
        };
        args.state.push(RandomGenerator { rng })?;
        Ok(1)
    })?;

    m.function("noise", |args| {
        let dimensions = args.len().clamp(1, 4);
        let mut c = [0.0; 4];
        for (i, c) in c.iter_mut().enumerate().take(dimensions) {
            *c = args.number(i + 1)?;
        }
        args.ret(match dimensions {
            1 => engine::noise1(c[0]),
            2 => engine::noise2(c[0], c[1]),
            3 => engine::noise3(c[0], c[1], c[2]),
            _ => engine::noise4(c[0], c[1], c[2], c[3]),
        })
    })?;

    m.function("gammaToLinear", |args| {
        convert_gamma(args, engine::gamma_to_linear)
    })?;
    m.function("linearToGamma", |args| {
        convert_gamma(args, engine::linear_to_gamma)
    })?;
    m.function("colorToBytes", |args| {
        convert_bytes(args, |c| (c.clamp(0.0, 1.0) * 255.0 + 0.5).floor())
    })?;
    m.function("colorFromBytes", |args| {
        convert_bytes(args, |b| ((b + 0.5).floor() / 255.0).clamp(0.0, 1.0))
    })?;

    m.function("isConvex", |args| {
        let polygon = args.vertices(1)?;
        args.ret(engine::is_convex(&polygon))
    })?;
    m.function("triangulate", |args| {
        let polygon = args.vertices(1)?;
        let triangles = engine::triangulate(&polygon).map_err(|e| args.named_error(e))?;
        let list = LuaApi::create_table(args.state)?;
        for (i, triangle) in triangles.iter().enumerate() {
            let coords = LuaApi::create_sequence_from(args.state, coordinates(triangle))?;
            list.raw_seti(i as i64 + 1, coords)?;
        }
        args.ret(list)
    })?;

    m.function("newBezierCurve", |args| {
        let points = args.vertices(1)?;
        push_curve(args, engine::BezierCurve { points })
    })?;
    m.function("newTransform", |args| {
        let matrix = placement(args, 1)?.matrix();
        push_transform(args, matrix)
    })?;

    m.finish(pg, "math")
}

/// The flat `x1, y1, x2, y2, ...` list for `points`.
fn coordinates(points: &[DVec2]) -> impl Iterator<Item = LuaValue> + '_ {
    points.iter().flat_map(|p| [number(p.x), number(p.y)])
}

// ---- random numbers ----

#[derive(LuaUserData)]
pub struct RandomGenerator {
    rng: Rng,
}

methods!(RandomGenerator {
    "random" => |args| random(args, Generator::This),
    "randomNormal" => |args| random_normal(args, Generator::This),
    "setSeed" => |args| set_seed(args, Generator::This),
    "getSeed" => |args| get_seed(args, Generator::This),
    "setState" => |args| set_state(args, Generator::This),
    "getState" => |args| get_state(args, Generator::This),
});

/// One of the functions shared by `pg.math` and `RandomGenerator`, like `random`.
type RandomFunction = fn(&mut Args, Generator) -> LuaResult<usize>;

/// Where a random function gets its generator: `pg.math`'s own, or `self` for a
/// `RandomGenerator` method.
#[derive(Clone, Copy)]
enum Generator<'a> {
    Global(&'a RefCell<Rng>),
    This,
}

impl Generator<'_> {
    /// The index of the first argument after the generator.
    fn first(self) -> usize {
        match self {
            Generator::Global(_) => 1,
            Generator::This => 2,
        }
    }

    fn with<R>(self, args: &mut Args, f: impl FnOnce(&mut Rng) -> R) -> LuaResult<R> {
        match self {
            Generator::Global(rng) => Ok(f(&mut rng.borrow_mut())),
            Generator::This => args.this(|g: &mut RandomGenerator| f(&mut g.rng)),
        }
    }
}

/// `random()`, `random(max)` or `random(min, max)`, with Love2D's formula for the integer ranges.
fn random(args: &mut Args, generator: Generator) -> LuaResult<usize> {
    let first = generator.first();
    let bounds = if args.get(first + 1).is_some() {
        let (low, high) = (args.number(first)?, args.number(first + 1)?);
        if low > high {
            return Err(args.arg_error(first + 1, "interval is empty"));
        }
        Some((low, high))
    } else if args.get(first).is_some() {
        let high = args.number(first)?;
        if high < 1.0 {
            return Err(args.arg_error(first, "interval is empty"));
        }
        Some((1.0, high))
    } else {
        None
    };
    let r = generator.with(args, Rng::random)?;
    match bounds {
        Some((low, high)) => args.ret(number((r * (high - low + 1.0)).floor() + low)),
        None => args.ret(r),
    }
}

fn random_normal(args: &mut Args, generator: Generator) -> LuaResult<usize> {
    let first = generator.first();
    let stddev = args.opt_number(first, 1.0)?;
    let mean = args.opt_number(first + 1, 0.0)?;
    let n = generator.with(args, |rng| rng.normal(stddev))?;
    args.ret(n + mean)
}

fn set_seed(args: &mut Args, generator: Generator) -> LuaResult<usize> {
    let seed = seed(args, generator.first())?;
    generator.with(args, |rng| rng.set_seed(seed))?;
    Ok(0)
}

/// Returns the seed's low and high 32 bits, as Love2D does.
fn get_seed(args: &mut Args, generator: Generator) -> LuaResult<usize> {
    let seed = generator.with(args, |rng| rng.seed())?;
    args.ret((i64::from(seed as u32), i64::from((seed >> 32) as u32)))
}

fn set_state(args: &mut Args, generator: Generator) -> LuaResult<usize> {
    let first = generator.first();
    let state = args.string(first)?;
    if generator.with(args, |rng| rng.set_state(&state))? {
        Ok(0)
    } else {
        Err(args.arg_error(first, format!("invalid random state '{state}'")))
    }
}

fn get_state(args: &mut Args, generator: Generator) -> LuaResult<usize> {
    let state = generator.with(args, |rng| rng.state())?;
    args.ret(state)
}

/// A seed given as one number, or as its low and high 32 bits.
fn seed(args: &mut Args, index: usize) -> LuaResult<u64> {
    if args.get(index + 1).is_some() {
        let low = seed_part(args, index)? as u32;
        let high = seed_part(args, index + 1)? as u32;
        Ok((u64::from(high) << 32) | u64::from(low))
    } else {
        seed_part(args, index)
    }
}

fn seed_part(args: &mut Args, index: usize) -> LuaResult<u64> {
    if let Some(i) = args.get(index).and_then(|v| v.as_integer_strict()) {
        return Ok(i as u64);
    }
    let n = args.number(index)?;
    if !n.is_finite() {
        return Err(args.arg_error(index, "invalid random seed"));
    }
    Ok(n as i64 as u64)
}

// ---- color ----

/// Up to four color components: the numbers from argument 1, or a table there.
fn components(args: &mut Args) -> LuaResult<Vec<f64>> {
    let mut values = Vec::with_capacity(4);
    if let Some(table) = args.table(1)? {
        for i in 1..=4 {
            let value: LuaValue = table.raw_geti(i)?;
            if value.is_nil() {
                break;
            }
            match value.as_number() {
                Some(n) => values.push(n),
                None => return Err(args.arg_error(1, "color components must be numbers")),
            }
        }
    } else {
        for i in 1..=args.len().min(4) {
            values.push(args.number(i)?);
        }
    }
    Ok(values)
}

/// Applies `convert` to each of the 1 to 3 color components given, clamped to [0, 1]. Alpha is
/// always linear, so a fourth component only gets clamped.
fn convert_gamma(args: &mut Args, convert: fn(f64) -> f64) -> LuaResult<usize> {
    let components = components(args)?;
    if components.is_empty() {
        return Err(args.type_error(1, "number"));
    }
    for (i, c) in components.iter().enumerate() {
        let c = c.clamp(0.0, 1.0);
        args.state
            .push(number(if i < 3 { convert(c) } else { c }))?;
    }
    Ok(components.len())
}

/// Applies `convert` to each of the 3 or 4 color components given.
fn convert_bytes(args: &mut Args, convert: fn(f64) -> f64) -> LuaResult<usize> {
    let components = components(args)?;
    if components.len() < 3 {
        return Err(match args.table(1)? {
            Some(_) => args.arg_error(1, "color table needs at least 3 numbers"),
            None => args.type_error(components.len() + 1, "number"),
        });
    }
    for c in &components {
        args.state.push(number(convert(*c)))?;
    }
    Ok(components.len())
}

// ---- Bézier curves ----

#[derive(LuaUserData)]
pub struct BezierCurve {
    curve: engine::BezierCurve,
}

/// `render` depths above this are almost certainly mistakes: 16 already gives 65536 segments
/// for each segment of the control polygon.
const MAX_DEPTH: i64 = 16;

methods!(BezierCurve {
    "getDegree" => |args| {
        let degree = curve(args, |c| c.degree())?;
        args.ret(degree)
    },
    "getDerivative" => |args| {
        let derivative = curve(args, |c| c.derivative())?.map_err(|e| args.named_error(e))?;
        push_curve(args, derivative)
    },
    "getControlPoint" => |args| {
        let i = args.integer(2)?;
        let point = curve(args, |c| c.point(i))?.map_err(|e| args.named_error(e))?;
        args.ret((number(point.x), number(point.y)))
    },
    "setControlPoint" => |args| {
        let i = args.integer(2)?;
        let point = dvec2(args.number(3)?, args.number(4)?);
        curve(args, |c| c.set_point(i, point))?.map_err(|e| args.named_error(e))?;
        Ok(0)
    },
    "insertControlPoint" => |args| {
        let point = dvec2(args.number(2)?, args.number(3)?);
        let i = args.opt_integer(4, -1)?;
        curve(args, |c| c.insert_point(i, point))?;
        Ok(0)
    },
    "removeControlPoint" => |args| {
        let i = args.integer(2)?;
        curve(args, |c| c.remove_point(i))?.map_err(|e| args.named_error(e))?;
        Ok(0)
    },
    "getControlPointCount" => |args| {
        let count = curve(args, |c| c.points.len() as i64)?;
        args.ret(count)
    },
    "translate" => |args| {
        let delta = dvec2(args.number(2)?, args.number(3)?);
        curve(args, |c| c.translate(delta))?;
        Ok(0)
    },
    "rotate" => |args| {
        let angle = args.number(2)?;
        let center = dvec2(args.opt_number(3, 0.0)?, args.opt_number(4, 0.0)?);
        curve(args, |c| c.rotate(angle, center))?;
        Ok(0)
    },
    "scale" => |args| {
        let factor = args.number(2)?;
        let center = dvec2(args.opt_number(3, 0.0)?, args.opt_number(4, 0.0)?);
        curve(args, |c| c.scale(factor, center))?;
        Ok(0)
    },
    "evaluate" => |args| {
        let t = args.number(2)?;
        let point = curve(args, |c| c.evaluate(t))?.map_err(|e| args.named_error(e))?;
        args.ret((number(point.x), number(point.y)))
    },
    "getSegment" => |args| {
        let (t1, t2) = (args.number(2)?, args.number(3)?);
        let segment = curve(args, |c| c.segment(t1, t2))?.map_err(|e| args.named_error(e))?;
        push_curve(args, segment)
    },
    "render" => |args| {
        let depth = depth(args, 2)?;
        let points = curve(args, |c| c.render(depth))?.map_err(|e| args.named_error(e))?;
        let list = LuaApi::create_sequence_from(args.state, coordinates(&points))?;
        args.ret(list)
    },
    "renderSegment" => |args| {
        let (start, end) = (args.number(2)?, args.number(3)?);
        let depth = depth(args, 4)?;
        let points = curve(args, |c| c.render_segment(start, end, depth))?
            .map_err(|e| args.named_error(e))?;
        let list = LuaApi::create_sequence_from(args.state, coordinates(&points))?;
        args.ret(list)
    },
});

/// Runs `f` on the curve a method was called on.
fn curve<R>(args: &mut Args, f: impl FnOnce(&mut engine::BezierCurve) -> R) -> LuaResult<R> {
    args.this(|c: &mut BezierCurve| f(&mut c.curve))
}

fn push_curve(args: &mut Args, curve: engine::BezierCurve) -> LuaResult<usize> {
    args.state.push(BezierCurve { curve })?;
    Ok(1)
}

/// The optional subdivision depth at `index`, 5 by default like Love2D.
fn depth(args: &mut Args, index: usize) -> LuaResult<u32> {
    let depth = args.opt_integer(index, 5)?;
    if depth > MAX_DEPTH {
        return Err(args.arg_error(index, format!("depth must be at most {MAX_DEPTH}")));
    }
    Ok(depth.max(0) as u32)
}

// ---- transforms ----

/// A 2D transformation matrix, which `pg.graphics` can apply. It's a 4x4 matrix, like Love2D's.
#[derive(LuaUserData, Clone)]
#[lua_impl(Mul)]
pub struct Transform {
    pub(super) matrix: Mat4,
}

/// `a * b` applies `b`, then `a`.
impl std::ops::Mul for Transform {
    type Output = Transform;

    fn mul(self, other: Transform) -> Transform {
        Transform {
            matrix: self.matrix * other.matrix,
        }
    }
}

/// Matrix layouts, as whether they're column-major.
pub(super) const LAYOUTS: &[(&str, bool)] = &[("row", false), ("column", true)];

methods!(Transform {
    "apply" => |args| {
        let other = args.userdata(2, "Transform", |t: &Transform| t.matrix)?;
        update(args, |m| *m *= other)
    },
    "clone" => |args| {
        let matrix = transform(args)?;
        push_transform(args, matrix)
    },
    "getMatrix" => |args| {
        // Row by row, though glam stores columns.
        for e in transform(args)?.transpose().to_cols_array() {
            args.state.push(number(e))?;
        }
        Ok(16)
    },
    "inverse" => |args| {
        let matrix = transform(args)?.inverse();
        push_transform(args, matrix)
    },
    "transformPoint" => |args| {
        let point = vec3(args.f32(2)?, args.f32(3)?, 0.0);
        let p = transform(args)?.transform_point3(point);
        args.ret((number(p.x), number(p.y)))
    },
    "inverseTransformPoint" => |args| {
        let point = vec3(args.f32(2)?, args.f32(3)?, 0.0);
        let p = transform(args)?.inverse().transform_point3(point);
        args.ret((number(p.x), number(p.y)))
    },
    "isAffine2DTransform" => |args| {
        // Nothing moves along or depends on z, and there's no projection.
        let e = transform(args)?.to_cols_array();
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let flat = [2, 3, 6, 7, 8, 9, 11, 14].iter().all(|&i| near(e[i], 0.0));
        args.ret(flat && near(e[10], 1.0) && near(e[15], 1.0))
    },
    "reset" => |args| update(args, |m| *m = Mat4::IDENTITY),
    "translate" => |args| {
        let (dx, dy) = (args.f32(2)?, args.f32(3)?);
        update(args, |m| *m *= Mat4::from_translation(vec3(dx, dy, 0.0)))
    },
    "rotate" => |args| {
        let angle = args.f32(2)?;
        update(args, |m| *m *= Mat4::from_rotation_z(angle))
    },
    "scale" => |args| {
        let sx = args.f32(2)?;
        let sy = args.opt_f32(3, sx)?;
        update(args, |m| *m *= Mat4::from_scale(vec3(sx, sy, 1.0)))
    },
    "shear" => |args| {
        let (kx, ky) = (args.f32(2)?, args.f32(3)?);
        update(args, |m| *m *= shear_matrix(kx, ky))
    },
    "setTransformation" => |args| {
        let matrix = placement(args, 2)?.matrix();
        update(args, |m| *m = matrix)
    },
    "setMatrix" => set_matrix,
});

/// The matrix of the Transform a method was called on.
fn transform(args: &mut Args) -> LuaResult<Mat4> {
    args.this(|t: &mut Transform| t.matrix)
}

/// Runs `f` on the matrix of the Transform a method was called on, then returns the Transform
/// for chaining.
fn update(args: &mut Args, f: impl FnOnce(&mut Mat4)) -> LuaResult<usize> {
    args.this(|t: &mut Transform| f(&mut t.matrix))?;
    args.ret_self()
}

fn push_transform(args: &mut Args, matrix: Mat4) -> LuaResult<usize> {
    args.state.push(Transform { matrix })?;
    Ok(1)
}

/// `setMatrix([layout,] ...)`, where `...` is 16 numbers, a table of them, or a table of four
/// tables of four. They're in row-major order unless `layout` is `"column"`.
fn set_matrix(args: &mut Args) -> LuaResult<usize> {
    let (column_major, start) = if args.get(2).is_some_and(|v| v.is_string()) {
        (args.option(2, "matrix layout", LAYOUTS)?, 3)
    } else {
        (false, 2)
    };
    let matrix = match matrix_table(args, start, column_major)? {
        Some(matrix) => matrix,
        None => {
            let mut elements = [0.0f32; 16];
            for (i, e) in elements.iter_mut().enumerate() {
                *e = args.f32(start + i)?;
            }
            from_elements(&elements, column_major)
        }
    };
    update(args, |m| *m = matrix)
}

/// The matrix in the table at `index`: 16 numbers, or four tables of four, in row-major order
/// unless `column_major`. `None` if that argument isn't a table.
pub(super) fn matrix_table(
    args: &mut Args,
    index: usize,
    column_major: bool,
) -> LuaResult<Option<Mat4>> {
    let Some(table) = args.table(index)? else {
        return Ok(None);
    };
    let nested = table.raw_geti::<LuaValue>(1)?.is_table();
    let mut elements = [0.0f32; 16];
    for (i, e) in elements.iter_mut().enumerate() {
        let (outer, inner) = (i as i64 / 4 + 1, i as i64 % 4 + 1);
        let value: LuaValue = if !nested {
            table.raw_geti(i as i64 + 1)?
        } else if table.raw_geti::<LuaValue>(outer)?.is_table() {
            table.raw_geti::<LuaTable>(outer)?.raw_geti(inner)?
        } else {
            LuaValue::nil()
        };
        match value.as_number() {
            Some(n) => *e = n as f32,
            None => return Err(args.arg_error(index, "matrix table must hold 16 numbers")),
        }
    }
    Ok(Some(from_elements(&elements, column_major)))
}

fn from_elements(elements: &[f32; 16], column_major: bool) -> Mat4 {
    // `from_cols_array` reads columns, so row-major input needs transposing.
    let matrix = Mat4::from_cols_array(elements);
    if column_major {
        matrix
    } else {
        matrix.transpose()
    }
}
