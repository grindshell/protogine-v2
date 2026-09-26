//! The engine side of `pg.math`: random numbers, noise, color conversion, polygons and Bézier
//! curves. The algorithms are Love2D's, so seeded sequences, noise and triangulations match it.
//! It has no Lua dependency.

use macroquad::math::DVec2;

// ---- random numbers ----

/// The seed of a new `RandomGenerator`, as in Love2D.
pub const DEFAULT_SEED: u64 = 0x0139_408D_CBBF_7A44;

/// Love2D's `RandomGenerator`: xorshift64*, seeded through Thomas Wang's 64-bit hash.
pub struct Rng {
    seed: u64,
    state: u64,
    /// Box–Muller makes normal numbers in pairs. This is the unused one, before scaling.
    spare_normal: Option<f64>,
}

impl Default for Rng {
    fn default() -> Rng {
        Rng::new(DEFAULT_SEED)
    }
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut rng = Rng {
            seed,
            state: 0,
            spare_normal: None,
        };
        rng.set_seed(seed);
        rng
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn set_seed(&mut self, seed: u64) {
        // Similar seeds give similar xorshift sequences, so hash the seed first. xorshift is stuck
        // at 0, so rehash until the state isn't.
        self.seed = seed;
        self.state = wang_hash(seed);
        while self.state == 0 {
            self.state = wang_hash(self.state);
        }
        self.spare_normal = None;
    }

    /// The state as Love2D formats it: `0x` and 16 hex digits.
    pub fn state(&self) -> String {
        format!("0x{:016x}", self.state)
    }

    /// Restores a state from [`Rng::state`]. Returns `false` if `state` isn't one.
    pub fn set_state(&mut self, state: &str) -> bool {
        let parsed = state
            .strip_prefix("0x")
            .filter(|hex| {
                (1..=16).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .and_then(|hex| u64::from_str_radix(hex, 16).ok())
            .filter(|&state| state != 0);
        let Some(parsed) = parsed else {
            return false;
        };
        self.state = parsed;
        self.spare_normal = None;
        true
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(2_685_821_657_736_338_717)
    }

    /// A uniformly distributed number in [0, 1).
    pub fn random(&mut self) -> f64 {
        // The top 52 bits become the mantissa of a number in [1, 2).
        f64::from_bits((0x3FF << 52) | (self.next() >> 12)) - 1.0
    }

    /// A normally distributed number with mean 0, by the Box–Muller transform.
    pub fn normal(&mut self, stddev: f64) -> f64 {
        if let Some(spare) = self.spare_normal.take() {
            return spare * stddev;
        }
        let r = (-2.0 * (1.0 - self.random()).ln()).sqrt();
        let phi = std::f64::consts::TAU * (1.0 - self.random());
        self.spare_normal = Some(r * phi.cos());
        r * phi.sin() * stddev
    }
}

fn wang_hash(mut key: u64) -> u64 {
    key = (!key).wrapping_add(key << 21);
    key ^= key >> 24;
    key = key.wrapping_add(key << 3).wrapping_add(key << 8);
    key ^= key >> 14;
    key = key.wrapping_add(key << 2).wrapping_add(key << 4);
    key ^= key >> 28;
    key.wrapping_add(key << 31)
}

// ---- noise ----
//
// Love2D uses Stefan Gustavson's public-domain noise: simplex noise in 1D and 2D, and classic
// Perlin noise in 3D and 4D. These are ports in double precision (Love2D computes in single
// precision), with Love2D's scale factors, mapped from [-1, 1] to [0, 1].

/// Ken Perlin's permutation of 0..=255.
const PERM: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69,
    142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219,
    203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175,
    74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230,
    220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76,
    132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173,
    186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206,
    59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163,
    70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232,
    178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162,
    241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204,
    176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141,
    128, 195, 78, 66, 215, 61, 156, 180,
];

fn perm(i: usize) -> usize {
    usize::from(PERM[i & 255])
}

/// The lattice cell containing `x`, wrapped to 0..=255, and the offset of `x` within it.
fn cell(x: f64) -> (usize, f64) {
    let floor = x.floor();
    ((floor as i64 & 255) as usize, x - floor)
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

fn signed(value: f64, negate: bool) -> f64 {
    if negate { -value } else { value }
}

/// 1D simplex noise, in [0, 1].
pub fn noise1(x: f64) -> f64 {
    let grad = |hash: usize, x: f64| {
        let magnitude = 1.0 + (hash & 7) as f64;
        signed(magnitude, hash & 8 != 0) * x
    };
    let (i, x0) = cell(x);
    let x1 = x0 - 1.0;
    let n0 = (1.0 - x0 * x0).powi(4) * grad(perm(i), x0);
    let n1 = (1.0 - x1 * x1).powi(4) * grad(perm(i + 1), x1);
    0.395 * (n0 + n1) * 0.5 + 0.5
}

/// 2D simplex noise, in [0, 1].
pub fn noise2(x: f64, y: f64) -> f64 {
    const F2: f64 = 0.366_025_403_784_438_6; // (sqrt(3) - 1) / 2
    const G2: f64 = 0.211_324_865_405_187_1; // (3 - sqrt(3)) / 6
    let grad = |hash: usize, x: f64, y: f64| {
        let (u, v) = if hash & 7 < 4 { (x, y) } else { (y, x) };
        signed(u, hash & 1 != 0) + signed(2.0 * v, hash & 2 != 0)
    };

    // Skew to find the simplex cell, then unskew its origin back.
    let skew = (x + y) * F2;
    let (i, j) = ((x + skew).floor(), (y + skew).floor());
    let unskew = (i + j) * G2;
    let (x0, y0) = (x - (i - unskew), y - (j - unskew));
    // The middle corner depends on which triangle of the cell the point is in.
    let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let corners = [
        (0, 0, x0, y0),
        (i1, j1, x0 - i1 as f64 + G2, y0 - j1 as f64 + G2),
        (1, 1, x0 - 1.0 + 2.0 * G2, y0 - 1.0 + 2.0 * G2),
    ];
    let (ii, jj) = ((i as i64 & 255) as usize, (j as i64 & 255) as usize);
    let n: f64 = corners
        .iter()
        .map(|&(di, dj, dx, dy)| {
            let t = 0.5 - dx * dx - dy * dy;
            if t < 0.0 {
                0.0
            } else {
                t.powi(4) * grad(perm(ii + di + perm(jj + dj)), dx, dy)
            }
        })
        .sum();
    45.23 * n * 0.5 + 0.5
}

/// 3D Perlin noise, in [0, 1].
pub fn noise3(x: f64, y: f64, z: f64) -> f64 {
    let grad = |hash: usize, x: f64, y: f64, z: f64| {
        let h = hash & 15;
        let u = if h < 8 { x } else { y };
        let v = match h {
            0..4 => y,
            12 | 14 => x,
            _ => z,
        };
        signed(u, h & 1 != 0) + signed(v, h & 2 != 0)
    };
    let ((ix, fx), (iy, fy), (iz, fz)) = (cell(x), cell(y), cell(z));
    let corner = |dx: usize, dy: usize, dz: usize| {
        let hash = perm(ix + dx + perm(iy + dy + perm(iz + dz)));
        grad(hash, fx - dx as f64, fy - dy as f64, fz - dz as f64)
    };
    let along_z = |dx, dy| lerp(fade(fz), corner(dx, dy, 0), corner(dx, dy, 1));
    let along_y = |dx| lerp(fade(fy), along_z(dx, 0), along_z(dx, 1));
    0.936 * lerp(fade(fx), along_y(0), along_y(1)) * 0.5 + 0.5
}

/// 4D Perlin noise, in [0, 1].
pub fn noise4(x: f64, y: f64, z: f64, w: f64) -> f64 {
    let grad = |hash: usize, x: f64, y: f64, z: f64, w: f64| {
        let h = hash & 31;
        let u = if h < 24 { x } else { y };
        let v = if h < 16 { y } else { z };
        let t = if h < 8 { z } else { w };
        signed(u, h & 1 != 0) + signed(v, h & 2 != 0) + signed(t, h & 4 != 0)
    };
    let ((ix, fx), (iy, fy), (iz, fz), (iw, fw)) = (cell(x), cell(y), cell(z), cell(w));
    let corner = |dx: usize, dy: usize, dz: usize, dw: usize| {
        let hash = perm(ix + dx + perm(iy + dy + perm(iz + dz + perm(iw + dw))));
        let offset = |f: f64, d: usize| f - d as f64;
        grad(
            hash,
            offset(fx, dx),
            offset(fy, dy),
            offset(fz, dz),
            offset(fw, dw),
        )
    };
    let along_w = |dx, dy, dz| lerp(fade(fw), corner(dx, dy, dz, 0), corner(dx, dy, dz, 1));
    let along_z = |dx, dy| lerp(fade(fz), along_w(dx, dy, 0), along_w(dx, dy, 1));
    let along_y = |dx| lerp(fade(fy), along_z(dx, 0), along_z(dx, 1));
    0.87 * lerp(fade(fx), along_y(0), along_y(1)) * 0.5 + 0.5
}

// ---- color ----

/// Converts an sRGB component to linear RGB.
pub fn gamma_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts a linear RGB component to sRGB.
pub fn linear_to_gamma(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// ---- polygons ----

/// Whether every corner of the polygon turns the same way. Straight corners count either way;
/// fewer than three points aren't a polygon.
pub fn is_convex(polygon: &[DVec2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut winding = 0.0;
    for i in 0..n {
        let (a, b, c) = (polygon[i], polygon[(i + 1) % n], polygon[(i + 2) % n]);
        let turn = (b - a).perp_dot(c - b);
        if turn * winding < 0.0 {
            return false;
        }
        if turn != 0.0 {
            winding = turn;
        }
    }
    true
}

/// Splits a simple polygon into triangles by ear clipping (Kong's algorithm, as in Love2D).
/// Fails if the polygon intersects itself.
pub fn triangulate(polygon: &[DVec2]) -> Result<Vec<[DVec2; 3]>, &'static str> {
    let n = polygon.len();
    if n < 3 {
        return Err("need at least three vertices");
    }
    if n == 3 {
        return Ok(vec![[polygon[0], polygon[1], polygon[2]]]);
    }

    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let corner =
        |prev: &[usize], next: &[usize], i: usize| (polygon[prev[i]], polygon[i], polygon[next[i]]);

    // The leftmost vertex is always convex, so its turn gives the polygon's winding. Walk the
    // polygon the way that makes it counterclockwise.
    let leftmost = (0..n)
        .min_by(|&i, &j| {
            let (a, b) = (polygon[i], polygon[j]);
            a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y))
        })
        .unwrap_or(0);
    let (a, b, c) = corner(&prev, &next, leftmost);
    if !is_ccw(a, b, c) {
        std::mem::swap(&mut next, &mut prev);
    }

    let mut concave: Vec<usize> = (0..n)
        .filter(|&i| {
            let (a, b, c) = corner(&prev, &next, i);
            !is_ccw(a, b, c)
        })
        .collect();

    let mut triangles = Vec::with_capacity(n - 2);
    let (mut remaining, mut current, mut skipped) = (n, 1, 0);
    while remaining > 3 {
        let (p, q) = (prev[current], next[current]);
        let (a, b, c) = (polygon[p], polygon[current], polygon[q]);
        let blocked = concave
            .iter()
            .any(|&i| i != p && i != current && i != q && in_triangle(polygon[i], a, b, c));
        if is_ccw(a, b, c) && !blocked {
            triangles.push([a, b, c]);
            next[p] = q;
            prev[q] = p;
            concave.retain(|&i| i != current);
            remaining -= 1;
            skipped = 0;
        } else {
            skipped += 1;
            if skipped > remaining {
                return Err("cannot triangulate the polygon (does it intersect itself?)");
            }
        }
        current = q;
    }
    let (a, b, c) = corner(&prev, &next, current);
    triangles.push([a, b, c]);
    Ok(triangles)
}

/// Whether `a`, `b`, `c` turn counterclockwise (in y-up terms), or not at all.
fn is_ccw(a: DVec2, b: DVec2, c: DVec2) -> bool {
    (b - a).perp_dot(c - a) >= 0.0
}

/// Whether `p` is inside triangle `abc` or on its edge.
fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let same_side = |p: DVec2, q: DVec2, from: DVec2, to: DVec2| {
        let edge = to - from;
        edge.perp_dot(p - from) * edge.perp_dot(q - from) >= 0.0
    };
    same_side(p, a, b, c) && same_side(p, b, a, c) && same_side(p, c, a, b)
}

// ---- Bézier curves ----

/// Love2D's `BezierCurve`: a Bézier curve of any degree, defined by its control points.
#[derive(Clone)]
pub struct BezierCurve {
    pub points: Vec<DVec2>,
}

pub const NO_POINTS: &str = "the curve has no control points";
pub const TOO_FEW_POINTS: &str = "the curve needs at least two control points";

impl BezierCurve {
    /// -1 for a curve with no points.
    pub fn degree(&self) -> i64 {
        self.points.len() as i64 - 1
    }

    /// The curve's derivative, a curve of one degree less.
    pub fn derivative(&self) -> Result<BezierCurve, &'static str> {
        if self.degree() < 1 {
            return Err("cannot derive a curve of degree < 1");
        }
        let degree = self.degree() as f64;
        let points = self.points.windows(2).map(|w| (w[1] - w[0]) * degree);
        Ok(BezierCurve {
            points: points.collect(),
        })
    }

    /// The position of a Lua index: counting from 1, or back from -1 for the last point, and
    /// wrapping around beyond that, as Love2D does. 0 is the first point.
    fn position(&self, index: i64) -> Result<usize, &'static str> {
        if self.points.is_empty() {
            return Err(NO_POINTS);
        }
        let index = if index > 0 { index - 1 } else { index };
        Ok(index.rem_euclid(self.points.len() as i64) as usize)
    }

    pub fn point(&self, index: i64) -> Result<DVec2, &'static str> {
        Ok(self.points[self.position(index)?])
    }

    pub fn set_point(&mut self, index: i64, point: DVec2) -> Result<(), &'static str> {
        let i = self.position(index)?;
        self.points[i] = point;
        Ok(())
    }

    pub fn remove_point(&mut self, index: i64) -> Result<(), &'static str> {
        let i = self.position(index)?;
        self.points.remove(i);
        Ok(())
    }

    /// Inserts `point` before the point at `index`, which may also be one past the end. As in
    /// Love2D, -1 inserts before the last point.
    pub fn insert_point(&mut self, index: i64, point: DVec2) {
        let n = self.points.len() as i64;
        let index = if index > 0 { index - 1 } else { index };
        let i = if n == 0 {
            0
        } else if index < 0 {
            index.rem_euclid(n)
        } else if index > n {
            (index - 1).rem_euclid(n) + 1
        } else {
            index
        };
        self.points.insert(i as usize, point);
    }

    pub fn translate(&mut self, delta: DVec2) {
        self.points.iter_mut().for_each(|p| *p += delta);
    }

    pub fn rotate(&mut self, angle: f64, center: DVec2) {
        let rotation = DVec2::from_angle(angle);
        for p in &mut self.points {
            *p = rotation.rotate(*p - center) + center;
        }
    }

    pub fn scale(&mut self, factor: f64, center: DVec2) {
        for p in &mut self.points {
            *p = (*p - center) * factor + center;
        }
    }

    /// The point at `t`, from 0 to 1, by de Casteljau's algorithm.
    pub fn evaluate(&self, t: f64) -> Result<DVec2, &'static str> {
        if !(0.0..=1.0).contains(&t) {
            return Err("the curve parameter must be between 0 and 1");
        }
        if self.points.len() < 2 {
            return Err(TOO_FEW_POINTS);
        }
        let mut points = self.points.clone();
        for step in 1..points.len() {
            for i in 0..points.len() - step {
                points[i] = points[i] * (1.0 - t) + points[i + 1] * t;
            }
        }
        Ok(points[0])
    }

    /// The part of the curve from `t1` to `t2`, as a curve of the same degree.
    pub fn segment(&self, t1: f64, t2: f64) -> Result<BezierCurve, &'static str> {
        if t1 < 0.0 || t2 > 1.0 {
            return Err("segment parameters must be between 0 and 1");
        }
        if t2 <= t1 {
            return Err("the segment's start must be before its end");
        }
        if self.points.len() < 2 {
            return Err(TOO_FEW_POINTS);
        }
        // Split at t2, then split the left part at t1 / t2. Its right part is the segment.
        let mut points = self.points.clone();
        let mut left = Vec::with_capacity(points.len());
        for step in 1..points.len() {
            left.push(points[0]);
            for i in 0..points.len() - step {
                points[i] = points[i].lerp(points[i + 1], t2);
            }
        }
        left.push(points[0]);

        let s = t1 / t2;
        let mut right = Vec::with_capacity(left.len());
        for step in 1..left.len() {
            right.push(left[left.len() - step]);
            for i in 0..left.len() - step {
                left[i] = left[i].lerp(left[i + 1], s);
            }
        }
        right.push(left[0]);
        right.reverse();
        Ok(BezierCurve { points: right })
    }

    /// Points along the curve, from subdividing the control polygon `depth` times.
    pub fn render(&self, depth: u32) -> Result<Vec<DVec2>, &'static str> {
        if self.points.len() < 2 {
            return Err(TOO_FEW_POINTS);
        }
        let mut points = self.points.clone();
        subdivide(&mut points, depth);
        Ok(points)
    }

    /// The rendered points between `start` and `end`, from 0 to 1, in either order.
    pub fn render_segment(
        &self,
        start: f64,
        end: f64,
        depth: u32,
    ) -> Result<Vec<DVec2>, &'static str> {
        if !(0.0..=1.0).contains(&start) || !(0.0..=1.0).contains(&end) {
            return Err("segment parameters must be between 0 and 1");
        }
        let points = self.render(depth)?;
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let n = points.len() as f64;
        let first = (start * n) as usize;
        let last = ((end * n + 0.5) as usize).min(points.len());
        if start == end || first >= last {
            return Ok(Vec::new());
        }
        Ok(points[first..last].to_vec())
    }
}

/// Subdivides a control polygon `depth` times, by de Casteljau's algorithm.
fn subdivide(points: &mut Vec<DVec2>, depth: u32) {
    let n = points.len();
    if depth == 0 || n < 2 {
        return;
    }
    // Splitting at 0.5 leaves the left half's control points along the first column of the
    // de Casteljau triangle and the right half's along its diagonal, back to front.
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for step in 1..n {
        left.push(points[0]);
        right.push(points[n - step]);
        for i in 0..n - step {
            points[i] = (points[i] + points[i + 1]) * 0.5;
        }
    }
    left.push(points[0]);
    right.push(points[0]);

    subdivide(&mut left, depth - 1);
    subdivide(&mut right, depth - 1);
    // Both halves share the split point, so drop it from the reversed right half.
    points.clear();
    points.extend(&left);
    points.extend(right.iter().rev().skip(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::math::dvec2;

    #[test]
    fn perm_is_a_permutation() {
        let mut seen = [false; 256];
        for &p in &PERM {
            seen[usize::from(p)] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn rng_is_deterministic_and_restorable() {
        let mut a = Rng::default();
        let mut b = Rng::new(DEFAULT_SEED);
        let first: Vec<f64> = (0..100).map(|_| a.random()).collect();
        assert!(first.iter().all(|r| (0.0..1.0).contains(r)));
        assert!(
            first
                .iter()
                .zip((0..100).map(|_| b.random()))
                .all(|(x, y)| *x == y)
        );

        let state = a.state();
        assert!(state.starts_with("0x") && state.len() == 18);
        let expected: Vec<f64> = (0..10).map(|_| a.random()).collect();
        assert!(b.set_state(&state));
        assert_eq!(expected, (0..10).map(|_| b.random()).collect::<Vec<_>>());

        for bad in ["", "0x", "12", "0xg1", "0x+1", "0x0", "0x00000000000000001"] {
            assert!(!b.set_state(bad), "{bad}");
        }
        assert_eq!(Rng::new(0).seed(), 0);
        assert_ne!(Rng::new(0).state(), Rng::new(1).state());
    }

    #[test]
    fn normal_numbers_have_the_right_spread() {
        let mut rng = Rng::new(7);
        let samples: Vec<f64> = (0..20_000).map(|_| rng.normal(2.0)).collect();
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let variance =
            samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / samples.len() as f64;
        assert!(mean.abs() < 0.05, "{mean}");
        assert!((variance.sqrt() - 2.0).abs() < 0.05, "{variance}");
    }

    #[test]
    fn noise_is_bounded_and_smooth() {
        // Every kind of noise is 0.5 on the lattice.
        assert_eq!(noise1(3.0), 0.5);
        assert_eq!(noise2(0.0, 0.0), 0.5);
        assert_eq!(noise3(1.0, -2.0, 5.0), 0.5);
        assert_eq!(noise4(1.0, 2.0, 3.0, -4.0), 0.5);

        let mut min: f64 = 1.0;
        let mut max: f64 = 0.0;
        for i in 0..2000 {
            let x = i as f64 * 0.137 - 100.0;
            for n in [
                noise1(x),
                noise2(x, x * 0.7),
                noise3(x, -x, x * 0.3),
                noise4(x, x * 0.5, -x, 1.5),
            ] {
                min = min.min(n);
                max = max.max(n);
            }
            // Nearby points give nearby values.
            assert!((noise2(x, 1.0) - noise2(x + 1e-4, 1.0)).abs() < 1e-2);
        }
        assert!(
            (0.0..0.2).contains(&min) && (0.8..=1.0).contains(&max),
            "{min} {max}"
        );
    }

    #[test]
    fn gamma_round_trips() {
        for i in 0..=100 {
            let c = i as f64 / 100.0;
            assert!((linear_to_gamma(gamma_to_linear(c)) - c).abs() < 1e-9);
        }
        assert!((gamma_to_linear(0.5) - 0.214_041).abs() < 1e-6);
    }

    fn polygon(coords: &[f64]) -> Vec<DVec2> {
        coords.chunks(2).map(|c| dvec2(c[0], c[1])).collect()
    }

    fn area(points: &[DVec2]) -> f64 {
        let n = points.len();
        (0..n)
            .map(|i| points[i].perp_dot(points[(i + 1) % n]))
            .sum::<f64>()
            .abs()
            / 2.0
    }

    #[test]
    fn convexity() {
        let square = polygon(&[0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]);
        assert!(is_convex(&square));
        let mut reversed = square.clone();
        reversed.reverse();
        assert!(is_convex(&reversed));
        // A straight corner doesn't count.
        assert!(is_convex(&polygon(&[
            0.0, 0.0, 5.0, 0.0, 10.0, 0.0, 10.0, 10.0
        ])));
        // An arrowhead, with its straight corner first.
        assert!(!is_convex(&polygon(&[
            0.0, 0.0, 5.0, 0.0, 10.0, 0.0, 5.0, 3.0, 5.0, 10.0
        ])));
        assert!(!is_convex(&square[..2]));
    }

    #[test]
    fn triangulates_concave_polygons() {
        // An L shape, in both windings.
        let mut shape = polygon(&[
            0.0, 0.0, 20.0, 0.0, 20.0, 10.0, 10.0, 10.0, 10.0, 20.0, 0.0, 20.0,
        ]);
        for _ in 0..2 {
            let triangles = triangulate(&shape).unwrap();
            assert_eq!(triangles.len(), shape.len() - 2);
            let total: f64 = triangles.iter().map(|t| area(t)).sum();
            assert!((total - area(&shape)).abs() < 1e-9);
            shape.reverse();
        }

        // Ear clipping gets stuck on some self-intersecting polygons (and not on others).
        let tangle = polygon(&[30.0, 20.0, 0.0, 30.0, 30.0, 10.0, 10.0, 30.0, 30.0, 30.0]);
        assert!(triangulate(&tangle).is_err());
        assert!(triangulate(&shape[..2]).is_err());
    }

    #[test]
    fn bezier_curves() {
        let curve = BezierCurve {
            points: polygon(&[0.0, 0.0, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0]),
        };
        assert_eq!(curve.degree(), 3);
        assert_eq!(curve.evaluate(0.0).unwrap(), dvec2(0.0, 0.0));
        assert_eq!(curve.evaluate(1.0).unwrap(), dvec2(10.0, 0.0));
        assert_eq!(curve.evaluate(0.5).unwrap(), dvec2(5.0, 7.5));
        assert!(curve.evaluate(1.5).is_err());

        // A segment traces the same points as the curve it came from.
        let segment = curve.segment(0.25, 0.75).unwrap();
        let (a, b) = (segment.evaluate(0.5).unwrap(), curve.evaluate(0.5).unwrap());
        assert!(a.distance(b) < 1e-9);

        let rendered = curve.render(3).unwrap();
        assert_eq!(rendered.len(), 3 * 8 + 1);
        assert_eq!(rendered[0], curve.points[0]);
        assert_eq!(rendered[rendered.len() - 1], curve.points[3]);
        assert!(rendered[12].distance(curve.evaluate(0.5).unwrap()) < 1e-9);
        assert_eq!(curve.render_segment(0.5, 0.5, 3).unwrap().len(), 0);
        assert_eq!(curve.render_segment(0.0, 0.5, 3).unwrap().len(), 13);

        let derivative = curve.derivative().unwrap();
        assert_eq!(
            derivative.points,
            polygon(&[0.0, 30.0, 30.0, 0.0, 0.0, -30.0])
        );
    }

    #[test]
    fn bezier_indices_wrap_like_love2d() {
        let mut curve = BezierCurve {
            points: polygon(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0]),
        };
        assert_eq!(curve.point(1).unwrap(), dvec2(1.0, 1.0));
        assert_eq!(curve.point(-1).unwrap(), dvec2(3.0, 3.0));
        assert_eq!(curve.point(0).unwrap(), dvec2(1.0, 1.0));
        assert_eq!(curve.point(5).unwrap(), dvec2(2.0, 2.0));

        curve.insert_point(-1, dvec2(9.0, 9.0));
        assert_eq!(curve.points[2], dvec2(9.0, 9.0));
        curve.insert_point(5, dvec2(8.0, 8.0));
        assert_eq!(curve.points[4], dvec2(8.0, 8.0));

        let mut empty = BezierCurve { points: Vec::new() };
        assert!(empty.point(1).is_err());
        empty.insert_point(-1, dvec2(4.0, 4.0));
        assert_eq!(empty.points, vec![dvec2(4.0, 4.0)]);
    }
}
