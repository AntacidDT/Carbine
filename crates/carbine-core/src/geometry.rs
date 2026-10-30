//! The convex geometry Carbine's colliders are made of, and the two tests that
//! read it: GJK for whether two solids meet, EPA for how deeply they meet.
//!
//! A solid is a handful of world-space points stretched by a radius, which is
//! every shape an author is offered: a box is its eight corners, a mesh is its
//! own vertices, a sphere is one point, a capsule is two. Both tests ask that
//! one question of a shape: how far does it reach in that direction? From the
//! answer GJK walks the two hulls toward each other, and EPA walks their
//! overlap outward to the shallowest way out.
//!
//! Owning the maths buys two things. A collider is never wider than the shape
//! the author drew, because the hull really is the hull. And an answer can be
//! checked before it moves anything: [`separates`] re-tests a contact by pushing
//! along it, so a suspect normal drops the pair for this frame instead of
//! throwing a body across the map.

use glam::Vec3;

/// Two points nearer than this are the same place, in meters.
const EPS: f32 = 1e-6;
/// The slack a contact is confirmed with, so shapes resting flush still read as
/// clear. Half a millimeter is well below anything an author can see.
const CHECK_SLOP: f32 = 5.0e-4;
/// The most probes either test spends on a pair before giving up on it.
const MAX_PROBES: usize = 32;
/// How many times the walk will swing out of a line or sheet through the origin
/// before it settles for the plane its probes proved.
const COLLAPSE_TRIES: usize = 6;
/// The most places kept for EPA's first four, which bounds the search for a
/// tetrahedron around the origin to a couple of thousand tries.
const MAX_SEED_POINTS: usize = 16;
/// The biggest polytope EPA will grow. A shape that needs more than this is not
/// a collider anyone authored, and a contact is dropped rather than chased.
const MAX_VERTICES: usize = 48;

/// A convex solid in world space: the hull of `points`, inflated by `radius`.
///
/// The radius stays apart from the points instead of being baked into them,
/// which is what keeps a sphere round and a capsule's ends hemispherical while
/// the tests below still only ever see a pile of points.
#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    points: Vec<Vec3>,
    radius: f32,
}

impl Solid {
    /// A solid from the points of its hull and the radius around them. An empty
    /// hull, or a radius that is not a number, is a shape that is nowhere, so it
    /// is refused here rather than checked for at every call.
    pub fn new(points: Vec<Vec3>, radius: f32) -> Option<Solid> {
        if points.is_empty()
            || points.iter().any(|point| !point.is_finite())
            || !radius.is_finite()
            || radius < 0.0
        {
            return None;
        }
        Some(Solid { points, radius })
    }

    /// A ball: one point, all of the shape in the radius.
    pub fn ball(center: Vec3, radius: f32) -> Option<Solid> {
        Solid::new(vec![center], radius)
    }

    /// A capsule: the segment between the two ends, inflated. The ends are the
    /// centres of its caps, so the solid reaches `radius` past both.
    pub fn capsule(ends: [Vec3; 2], radius: f32) -> Option<Solid> {
        Solid::new(ends.to_vec(), radius)
    }

    /// The same solid shifted bodily by `offset`.
    pub fn moved(&self, offset: Vec3) -> Solid {
        Solid {
            points: self.points.iter().map(|point| *point + offset).collect(),
            radius: self.radius,
        }
    }

    /// The hull point that reaches furthest in `dir`. The radius deliberately
    /// stays out of the answer, because the tests add the two radii back once.
    fn hull_support(&self, dir: Vec3) -> Vec3 {
        let mut best = self.points[0];
        let mut reach = best.dot(dir);
        for point in &self.points[1..] {
            let candidate = point.dot(dir);
            if candidate > reach {
                reach = candidate;
                best = *point;
            }
        }
        best
    }

    /// The middle of the hull, used only to pick a first search direction and to
    /// part two shapes that have no face to lean on.
    fn centroid(&self) -> Vec3 {
        self.points.iter().sum::<Vec3>() / self.points.len() as f32
    }

    /// The axis-aligned box the solid sits in, radius included. Cheap enough to
    /// ask first, so two shapes well apart never pay for a walk.
    pub fn aabb(&self) -> (Vec3, Vec3) {
        let mut min = self.points[0];
        let mut max = self.points[0];
        for point in &self.points[1..] {
            min = min.min(*point);
            max = max.max(*point);
        }
        (
            min - Vec3::splat(self.radius),
            max + Vec3::splat(self.radius),
        )
    }

    /// How many corners the hull is made of. A mesh collider carries one per
    /// vertex of the body it was cut from.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Whether two solids' axis-aligned boxes touch.
    pub fn in_boxes(a: &Solid, b: &Solid) -> bool {
        let (a_min, a_max) = a.aabb();
        let (b_min, b_max) = b.aabb();
        a_min.x <= b_max.x
            && b_min.x <= a_max.x
            && a_min.y <= b_max.y
            && b_min.y <= a_max.y
            && a_min.z <= b_max.z
            && b_min.z <= a_max.z
    }
}

/// How two solids overlap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// A unit vector out of the first solid and into the second. Pushing the
    /// first back along it, or the second on along it, is the way apart.
    pub normal: Vec3,
    /// How far along `normal` they overlap, in meters.
    pub depth: f32,
}

/// Whether the two solids ride into each other far enough to be worth a push.
/// Shapes that merely touch are not.
pub fn overlaps(a: &Solid, b: &Solid) -> bool {
    contact(a, b).is_some_and(|hit| hit.depth > EPS)
}

/// The shallowest way the two solids can be pulled apart, or `None` while they
/// still hold their distance.
pub fn contact(a: &Solid, b: &Solid) -> Option<Contact> {
    if !Solid::in_boxes(a, b) {
        return None;
    }
    let reach = a.radius + b.radius;
    match hulls(a, b) {
        // The hulls keep their distance, so any overlap left is the one their
        // radii reach into each other.
        Hull::Apart { gap } => reached_through(a, b, gap),
        // The hulls are through each other, so the way out is a face of the
        // difference between them. EPA grows that face when it can; when the
        // difference is too flat to grow from, the plane the walk already proved
        // stands in its place, and only a pair with neither falls back to the
        // middles.
        Hull::Joined { seed, plane } => {
            let grown = seed.and_then(|seed| deep_out(a, b, &seed));
            let hull = match (grown, plane) {
                (Some(face), Some(proved)) => Some(if face.depth <= proved.depth {
                    face
                } else {
                    proved
                }),
                (face, proved) => face.or(proved),
            };
            match hull {
                Some(hull) => Some(Contact {
                    normal: hull.normal,
                    depth: hull.depth + reach,
                }),
                None => part_by_centres(a, b, reach),
            }
        }
    }
}

/// Whether [`contact`]'s answer is one the solver may push on: moving the first
/// solid out along it really does clear the pair. `false` means the maths went
/// wrong somewhere, and the safe answer is to leave both alone this frame.
pub fn separates(a: &Solid, b: &Solid, contact: &Contact) -> bool {
    if !contact.normal.is_finite()
        || contact.normal.length_squared() < 0.5
        || !contact.depth.is_finite()
        || contact.depth < 0.0
    {
        return false;
    }
    let pushed_out = a.moved(-contact.normal * (contact.depth + CHECK_SLOP));
    !overlaps(&pushed_out, b)
}

/// The overlap the two radii account for, given the nearest the hulls come to
/// each other. `gap` is that nearest point of the difference: its length is how
/// far apart the hulls are and its direction runs from the second solid's hull
/// toward the first's.
fn reached_through(a: &Solid, b: &Solid, gap: Vec3) -> Option<Contact> {
    let distance = gap.length();
    let reach = a.radius + b.radius;
    if distance >= reach {
        return None;
    }
    if distance < EPS {
        return part_by_centres(a, b, reach);
    }
    Some(Contact {
        normal: -gap / distance,
        depth: reach - distance,
    })
}

/// A contact aimed along the line from one middle to the other, for when the
/// shapes present no face. The least precise answer here, and the only one that
/// can be given when nothing better exists.
fn part_by_centres(a: &Solid, b: &Solid, reach: f32) -> Option<Contact> {
    if reach <= EPS {
        return None;
    }
    let apart = b.centroid() - a.centroid();
    let distance = apart.length();
    let normal = if distance < EPS {
        Vec3::Z
    } else {
        apart / distance
    };
    Some(Contact {
        normal,
        depth: reach,
    })
}

/// What GJK found out about the two hulls.
enum Hull {
    /// The hulls hold apart, and `gap` is the point of their difference nearest
    /// the origin.
    Apart { gap: Vec3 },
    /// The hulls are through each other. EPA grows out from `seed`, the four
    /// points found around the origin, when the walk got that far. Whether or not
    /// it did, `plane` is the shallowest way out the probes have proved, so a
    /// pair with no clean face to grow from still has an answer worth pushing on.
    Joined {
        seed: Option<[Vec3; 4]>,
        plane: Option<Contact>,
    },
}

/// GJK: walk the difference of the two hulls from where they start apart toward
/// the origin, keeping only the points that still hold the nearest place.
fn hulls(a: &Solid, b: &Solid) -> Hull {
    let start = a.centroid() - b.centroid();
    let mut direction = if start.length_squared() < EPS {
        Vec3::X
    } else {
        start
    };
    let mut simplex = vec![difference(a, b, direction)];
    // Every distinct place the walk has stood on, kept in case the walk itself
    // gets stuck and EPA still needs four points to grow from.
    let mut kept = simplex.clone();
    let mut plane: Option<Contact> = None;
    let mut collapsed = 0;

    for _ in 0..MAX_PROBES {
        let (gap, enclosed) = nearest(&mut simplex);
        if enclosed {
            return joined(a, b, &simplex, &kept, plane);
        }
        let distance = gap.length();
        if distance < EPS {
            // The nearest place is the origin itself: the points so far lie in a
            // line or a sheet through it, which the region tests keep snapping
            // back to. Swing off it, so the walk gets a third dimension to work
            // in and a tetrahedron for EPA.
            if collapsed >= COLLAPSE_TRIES {
                return joined(a, b, &simplex, &kept, plane);
            }
            collapsed += 1;
            direction = swing(&simplex, collapsed);
            let candidate = difference(a, b, direction);
            prove(&mut plane, direction, candidate);
            keep(&mut kept, candidate);
            if simplex.len() < 4 && !holds_point(&simplex, candidate) {
                simplex.push(candidate);
            }
            continue;
        }
        // Search from the nearest place straight at the origin.
        direction = -gap;
        let candidate = difference(a, b, direction);
        prove(&mut plane, direction, candidate);
        // Nothing reaches past the plane through the nearest place, so that place
        // is as close as the two hulls ever get.
        if candidate.dot(direction) - gap.dot(direction) <= EPS * distance {
            return Hull::Apart { gap };
        }
        if holds_point(&simplex, candidate) {
            return Hull::Apart { gap };
        }
        simplex.push(candidate);
        keep(&mut kept, candidate);
    }

    let (gap, enclosed) = nearest(&mut simplex);
    if enclosed {
        return joined(a, b, &simplex, &kept, plane);
    }
    Hull::Apart { gap }
}

/// Four points to start EPA from, when the walk has four around the origin, and
/// the proof it settled for whatever else it found.
fn joined(a: &Solid, b: &Solid, simplex: &[Vec3], kept: &[Vec3], plane: Option<Contact>) -> Hull {
    let mut settled = plane;
    let seed = <[Vec3; 4]>::try_from(simplex.to_vec())
        .ok()
        .filter(encloses)
        .or_else(|| reach_around(a, b, kept, &mut settled));
    Hull::Joined {
        seed,
        plane: settled,
    }
}

/// Another place the walk stood on, so long as it is a place it has not had.
fn keep(kept: &mut Vec<Vec3>, point: Vec3) {
    if kept.len() < MAX_SEED_POINTS && !holds_point(kept, point) {
        kept.push(point);
    }
}

/// When the walk is stuck in a flat place the shapes still overlap, so some four
/// of their difference's points hold the origin. Reach for the fourteen ways the
/// difference stretches along the axes and the corners, add them to what the walk
/// kept, and take the first four that enclose it. Each reach also proves a plane
/// of its own, so even a pair left without a seed has a better way out than the
/// few directions the walk happened to try.
fn reach_around(
    a: &Solid,
    b: &Solid,
    kept: &[Vec3],
    plane: &mut Option<Contact>,
) -> Option<[Vec3; 4]> {
    let mut corners: Vec<Vec3> = kept.to_vec();
    corners.truncate(MAX_SEED_POINTS);
    let mut reaches = vec![Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X, -Vec3::Y, -Vec3::Z];
    for x in [1.0, -1.0] {
        for y in [1.0, -1.0] {
            for z in [1.0, -1.0] {
                reaches.push(Vec3::new(x, y, z));
            }
        }
    }
    for dir in reaches {
        let point = difference(a, b, dir);
        prove(plane, dir, point);
        if corners.len() < MAX_SEED_POINTS && !holds_point(&corners, point) {
            corners.push(point);
        }
    }

    for (first, a_point) in corners.iter().enumerate() {
        for (second, b_point) in corners.iter().enumerate().skip(first + 1) {
            for (third, c_point) in corners.iter().enumerate().skip(second + 1) {
                for d_point in corners.iter().skip(third + 1) {
                    let tetra = [*a_point, *b_point, *c_point, *d_point];
                    if encloses(&tetra) {
                        return Some(tetra);
                    }
                }
            }
        }
    }
    None
}

/// Whether four points hold the origin strictly inside the tetrahedron they
/// form: the origin must stay on the same side of every face as the vertex that
/// face does not touch, and off every face. A seed the origin lies on the skin
/// of has a face at zero distance, and EPA would read that as no overlap.
fn encloses(tetra: &[Vec3; 4]) -> bool {
    for (face, skip) in [
        ([0u8, 1, 2], 3usize),
        ([0, 1, 3], 2),
        ([0, 2, 3], 1),
        ([1, 2, 3], 0),
    ] {
        let a = tetra[face[0] as usize];
        let normal = (tetra[face[1] as usize] - a).cross(tetra[face[2] as usize] - a);
        let length = normal.length();
        if length < EPS {
            return false;
        }
        let normal = normal / length;
        if normal.dot(-a) * normal.dot(tetra[skip] - a) <= EPS {
            return false;
        }
    }
    true
}

/// Whether the walk has already stood on this exact point.
fn holds_point(simplex: &[Vec3], point: Vec3) -> bool {
    simplex
        .iter()
        .any(|held| (held - point).length_squared() < EPS * EPS)
}

/// Every probe proves a plane: nothing of the difference reaches past the support
/// point in the direction probed, so pushing the first hull out along that plane
/// by its distance separates the pair. The shallowest such proof is what the walk
/// can promise even when EPA has no polytope to grow.
fn prove(plane: &mut Option<Contact>, dir: Vec3, point: Vec3) {
    let unit = dir.normalize_or_zero();
    if unit.length_squared() < 0.5 {
        return;
    }
    let depth = point.dot(unit).max(0.0);
    if plane.is_none_or(|held| depth < held.depth) {
        *plane = Some(Contact {
            normal: unit,
            depth,
        });
    }
}

/// A direction that leaves the flat place the walk is stuck in, swung around it a
/// little each time, so a second collapse cannot repeat the first probe and spin.
fn swing(simplex: &[Vec3], round: usize) -> Vec3 {
    if simplex.len() >= 3 {
        let sheet = (simplex[1] - simplex[0])
            .cross(simplex[2] - simplex[0])
            .normalize_or_zero();
        if sheet.length_squared() > 0.5 {
            // Off the sheet is the only way to a fourth dimension, and either
            // side will do, so take turns.
            return if round.is_multiple_of(2) {
                -sheet
            } else {
                sheet
            };
        }
    }
    if simplex.len() >= 2 {
        let along = (simplex[1] - simplex[0]).normalize_or_zero();
        if along.length_squared() > 0.5 {
            let [x, y, z] = along.abs().to_array();
            // The basis axis least along the line cannot collapse a cross
            // product with it.
            let spin = if x <= y && x <= z {
                Vec3::X
            } else if y <= z {
                Vec3::Y
            } else {
                Vec3::Z
            };
            let off = along.cross(spin).normalize_or_zero();
            if off.length_squared() > 0.5 {
                let angle = round as f32 * 0.9;
                let swung =
                    (off * angle.cos() + along.cross(off) * angle.sin()).normalize_or_zero();
                if swung.length_squared() > 0.5 {
                    return swung;
                }
            }
            return spin;
        }
    }
    Vec3::Z
}

/// The farthest point of the Minkowski difference in `dir`: as far as `a`
/// reaches that way, less as far as `b` reaches the other way.
fn difference(a: &Solid, b: &Solid, dir: Vec3) -> Vec3 {
    a.hull_support(dir) - b.hull_support(-dir)
}

/// The point of the simplex nearest the origin, keeping only the vertices that
/// hold that place, and reporting when the origin lies inside all four.
fn nearest(simplex: &mut Vec<Vec3>) -> (Vec3, bool) {
    match simplex.len() {
        0 | 1 => (simplex.first().copied().unwrap_or(Vec3::ZERO), false),
        2 => on_segment(simplex),
        3 => on_triangle(simplex),
        _ => on_tetrahedron(simplex),
    }
}

fn on_segment(simplex: &mut Vec<Vec3>) -> (Vec3, bool) {
    let (a, b) = (simplex[0], simplex[1]);
    let ab = b - a;
    let along = (-a).dot(ab) / ab.length_squared().max(EPS);
    if along <= 0.0 {
        simplex.truncate(1);
        return (a, false);
    }
    if along >= 1.0 {
        *simplex = vec![b];
        return (b, false);
    }
    (a + ab * along, false)
}

/// The closest point of a triangle to the origin, by Voronoi region: the origin
/// lies against a vertex, an edge, or the face, and the simplex is cut back to
/// the points that hold that region.
fn on_triangle(simplex: &mut Vec<Vec3>) -> (Vec3, bool) {
    let (a, b, c) = (simplex[0], simplex[1], simplex[2]);
    let ab = b - a;
    let ac = c - a;
    let ao = -a;

    let along_ab = ab.dot(ao);
    let along_ac = ac.dot(ao);
    if along_ab <= 0.0 && along_ac <= 0.0 {
        *simplex = vec![a];
        return (a, false);
    }

    let bo = -b;
    let bc = c - b;
    let b_ac = ac.dot(bo);
    let b_ab = ab.dot(bo);
    if b_ab >= 0.0 && b_ac <= b_ab {
        *simplex = vec![b];
        return (b, false);
    }

    // The edge AB: the origin projects along it, past neither end.
    let vc = along_ab * b_ac - b_ab * along_ac;
    if vc <= 0.0 && along_ab >= 0.0 && b_ab <= 0.0 {
        *simplex = vec![a, b];
        return (a + ab * (along_ab / (along_ab - b_ab)), false);
    }

    let co = -c;
    let c_ab = ab.dot(co);
    let c_ac = ac.dot(co);
    if c_ac >= 0.0 && c_ab <= c_ac {
        *simplex = vec![c];
        return (c, false);
    }

    // The edge AC.
    let vb = c_ab * along_ac - along_ab * c_ac;
    if vb <= 0.0 && along_ac >= 0.0 && c_ac <= 0.0 {
        *simplex = vec![a, c];
        return (a + ac * (along_ac / (along_ac - c_ac)), false);
    }

    // The edge BC.
    let va = b_ab * c_ac - c_ab * b_ac;
    if va <= 0.0 && (b_ac - b_ab) >= 0.0 && (c_ab - c_ac) >= 0.0 {
        *simplex = vec![b, c];
        let along = (b_ac - b_ab) / ((b_ac - b_ab) + (c_ab - c_ac));
        return (b + bc * along, false);
    }

    // The face. A triangle flattened into a line holds no face, and the nearest
    // of its three edges is the honest answer.
    let denominator = va + vb + vc;
    let normal = ab.cross(ac);
    if normal.length_squared() < EPS || denominator.abs() < EPS {
        return flattest_edge(simplex, a, b, c);
    }
    let v = vb / denominator;
    let w = vc / denominator;
    *simplex = vec![a, b, c];
    (a + ab * v + ac * w, false)
}

/// Where three points have collapsed onto one line: the nearest of their edges.
fn flattest_edge(simplex: &mut Vec<Vec3>, a: Vec3, b: Vec3, c: Vec3) -> (Vec3, bool) {
    let mut best = (vec![a], a);
    for (kept, (from, to)) in [
        (vec![a, b], (a, b)),
        (vec![b, c], (b, c)),
        (vec![a, c], (a, c)),
    ] {
        let (point, _) = closest_on_segment(from, to);
        if point.length_squared() < best.1.length_squared() {
            best = (kept, point);
        }
    }
    *simplex = best.0;
    (best.1, false)
}

/// The nearest place on the segment from `a` to `b` to the origin, and how far
/// along it it sits.
fn closest_on_segment(a: Vec3, b: Vec3) -> (Vec3, f32) {
    let ab = b - a;
    let along = (-a).dot(ab) / ab.length_squared().max(EPS);
    (a + ab * along.clamp(0.0, 1.0), along)
}

/// The origin against a tetrahedron: inside it, or out through one of the three
/// faces the newest point opened, whose opposite vertex is then dropped.
fn on_tetrahedron(simplex: &mut Vec<Vec3>) -> (Vec3, bool) {
    let a = simplex[0];
    let (b, c, d) = (simplex[1], simplex[2], simplex[3]);
    let (ab, ac, ad) = (b - a, c - a, d - a);
    let ao = -a;

    // Each face's normal turned away from the vertex it does not touch, so that
    // a positive reach toward the origin means the origin has left that face.
    let abc = turn_away(ac.cross(ab), ad);
    let acd = turn_away(ad.cross(ac), ab);
    let adb = turn_away(ab.cross(ad), ac);

    if abc.dot(ao) > 0.0 {
        *simplex = vec![a, c, b];
        return on_triangle(simplex);
    }
    if acd.dot(ao) > 0.0 {
        *simplex = vec![a, d, c];
        return on_triangle(simplex);
    }
    if adb.dot(ao) > 0.0 {
        *simplex = vec![a, b, d];
        return on_triangle(simplex);
    }
    (Vec3::ZERO, true)
}

/// `normal`, flipped when it points along `inside` rather than away from it.
fn turn_away(normal: Vec3, inside: Vec3) -> Vec3 {
    if normal.dot(inside) > 0.0 {
        -normal
    } else {
        normal
    }
}

/// EPA: grow a polytope of the difference face by face, always probing the face
/// the origin is nearest, until nothing reaches past it. That face is the
/// shallowest way the two hulls can part, and the contact it gives is the hulls'
/// own, before either radius is added back.
fn deep_out(a: &Solid, b: &Solid, seed: &[Vec3; 4]) -> Option<Contact> {
    let mut verts: Vec<Vec3> = seed.to_vec();
    let mut faces: Vec<[u8; 3]> = Vec::with_capacity(8);
    for triangle in [[0u8, 1, 2], [0, 2, 3], [0, 3, 1], [1, 3, 2]] {
        faces.push(wind(&verts, triangle).ok()?);
    }

    for _ in 0..MAX_PROBES {
        // The face the origin is nearest is the only one worth probing.
        let mut shallowest: Option<(Vec3, f32)> = None;
        for face in &faces {
            let Some((normal, distance)) = plane(&verts, *face) else {
                continue;
            };
            if shallowest.is_none_or(|(_, so_far)| distance < so_far) {
                shallowest = Some((normal, distance));
            }
        }
        let (normal, distance) = shallowest?;
        // The origin belongs strictly inside the polytope. A face level with it
        // means the seed was a sheet, not a solid, so EPA can read nothing here.
        if distance <= EPS {
            return None;
        }

        let candidate = difference(a, b, normal);
        // Nothing reaches past the face, so the face is the way out. The face
        // looks away from the origin, which is out of the first hull and into
        // the second, exactly as the disjoint case reports it.
        if candidate.dot(normal) - distance < EPS
            || verts
                .iter()
                .any(|vertex| (*vertex - candidate).length_squared() < EPS * EPS)
        {
            return Some(Contact {
                normal,
                depth: distance,
            });
        }
        if verts.len() >= MAX_VERTICES {
            return None;
        }

        let fresh = verts.len() as u8;
        verts.push(candidate);

        // Take back every face the new point looks at, and stitch the hole its
        // outline leaves to that point.
        let visible: Vec<bool> = faces
            .iter()
            .map(|face| {
                plane(&verts, *face).is_some_and(|(normal, _)| {
                    normal.dot(candidate - verts[face[0] as usize]) > 0.0
                })
            })
            .collect();
        let kept: Vec<[u8; 3]> = faces
            .iter()
            .zip(&visible)
            .filter(|(_, seen)| !**seen)
            .map(|(face, _)| *face)
            .collect();
        // An edge of a taken-back face is the rim of the hole only where a face
        // that stayed still holds the same two points. An edge shared between two
        // taken-back faces lies inside the hole, and a new face through it would
        // fold the polytope back on itself.
        let mut horizon: Vec<[u8; 2]> = Vec::with_capacity(8);
        for (face, seen) in faces.iter().zip(&visible) {
            if !*seen {
                continue;
            }
            for pair in [[0u8, 1], [1, 2], [2, 0]] {
                let (from, to) = (face[pair[0] as usize], face[pair[1] as usize]);
                let shared = kept
                    .iter()
                    .any(|other| holds(*other, from) && holds(*other, to));
                if shared {
                    horizon.push([from, to]);
                }
            }
        }
        faces = kept;
        faces.extend(
            horizon
                .into_iter()
                .filter_map(|edge| wind(&verts, [edge[0], edge[1], fresh]).ok()),
        );
        if faces.is_empty() {
            return None;
        }
    }
    None
}

/// The plane of a face: its unit normal, turned away from the origin the
/// polytope is built around, and how far behind it the origin sits. Zero means
/// the origin is level with the face, which is shapes touching rather than
/// overlapping, and a three-point line is no plane to trust.
fn plane(verts: &[Vec3], face: [u8; 3]) -> Option<(Vec3, f32)> {
    let a = verts[face[0] as usize];
    let normal = (verts[face[1] as usize] - a).cross(verts[face[2] as usize] - a);
    let length = normal.length();
    if length < EPS {
        return None;
    }
    let unit = normal / length;
    let unit = if unit.dot(a) < 0.0 { -unit } else { unit };
    Some((unit, unit.dot(a)))
}

/// The same three points, ordered so their plane faces away from the origin.
fn wind(verts: &[Vec3], face: [u8; 3]) -> Result<[u8; 3], ()> {
    let a = verts[face[0] as usize];
    let normal = (verts[face[1] as usize] - a).cross(verts[face[2] as usize] - a);
    if normal.length_squared() < EPS {
        return Err(());
    }
    Ok(if normal.dot(a) < 0.0 {
        [face[0], face[2], face[1]]
    } else {
        face
    })
}

/// Whether a face lists one of these vertices.
fn holds(face: [u8; 3], vertex: u8) -> bool {
    face[0] == vertex || face[1] == vertex || face[2] == vertex
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eight corners of an axis-aligned box, which is all a box collider is.
    fn box_hull(centre: Vec3, half: Vec3) -> Solid {
        let mut corners = Vec::with_capacity(8);
        for x in [centre.x - half.x, centre.x + half.x] {
            for y in [centre.y - half.y, centre.y + half.y] {
                for z in [centre.z - half.z, centre.z + half.z] {
                    corners.push(Vec3::new(x, y, z));
                }
            }
        }
        Solid::new(corners, 0.0).unwrap()
    }

    /// Whether a normal is the x axis: the shape parted along it, and nothing else.
    fn level(axes: [f32; 3]) -> bool {
        (axes[0] - 1.0).abs() < 1e-4 && axes[1].abs() < 1e-4 && axes[2].abs() < 1e-4
    }

    /// Whether the pair parts along `want`, which is read out of the first
    /// solid: out of it and into the second.
    fn faces(hit: &Contact, want: Vec3) -> bool {
        (hit.normal - want).length() < 1e-4
    }

    /// The answer read the other way round: the same overlap, facing the other
    /// way. A pair that only agrees one way is a pair the solver leans on.
    #[test]
    fn both_ways_of_reading_a_pair_agree() {
        let a = box_hull(Vec3::ZERO, Vec3::splat(1.0));
        let b = Solid::ball(Vec3::new(0.8, 0.4, 0.3), 1.0).unwrap();

        let forward = contact(&a, &b).unwrap();
        let back = contact(&b, &a).unwrap();
        assert!((forward.normal + back.normal).length() < 1e-4);
        assert!((forward.depth - back.depth).abs() < 1e-4);
    }

    #[test]
    fn two_balls_overlap_by_exactly_their_radii() {
        let a = Solid::ball(Vec3::ZERO, 1.0).unwrap();
        let b = Solid::ball(Vec3::new(1.5, 0.0, 0.0), 1.0).unwrap();

        let hit = contact(&a, &b).unwrap();
        assert!(level(hit.normal.to_array()));
        assert!((hit.depth - 0.5).abs() < 1e-4, "depth {}", hit.depth);
        assert!(separates(&a, &b, &hit));
    }

    #[test]
    fn balls_that_hold_their_distance_are_left_alone() {
        let a = Solid::ball(Vec3::ZERO, 1.0).unwrap();
        let b = Solid::ball(Vec3::new(2.5, 0.0, 0.0), 1.0).unwrap();

        assert_eq!(contact(&a, &b), None);
        assert!(!overlaps(&a, &b));
    }

    #[test]
    fn two_boxes_part_face_to_face() {
        let a = box_hull(Vec3::ZERO, Vec3::splat(1.0));
        let b = box_hull(Vec3::new(1.8, 0.0, 0.0), Vec3::splat(1.0));

        let hit = contact(&a, &b).unwrap();
        assert!(level(hit.normal.to_array()));
        assert!((hit.depth - 0.2).abs() < 1e-4, "depth {}", hit.depth);
        assert!(separates(&a, &b, &hit));
        assert!(
            !overlaps(&a.moved(-hit.normal * hit.depth), &b),
            "one push is the whole way out"
        );
    }

    #[test]
    fn corner_against_corner_takes_the_shallowest_way_out() {
        let a = box_hull(Vec3::ZERO, Vec3::splat(1.0));
        let b = box_hull(Vec3::new(1.6, 1.6, 0.0), Vec3::splat(1.0));

        let hit = contact(&a, &b).unwrap();
        // Either axis is as shallow as the other, and the walk picks one.
        assert!(
            (hit.depth - 0.4).abs() < 1e-3,
            "depth {} should be one axis, not the diagonal",
            hit.depth
        );
        assert!(hit.normal.abs().max_element() > 0.99);
        assert!(separates(&a, &b, &hit));
    }

    #[test]
    fn a_ball_above_a_wall_is_pushed_straight_off_its_face() {
        let ball = Solid::ball(Vec3::new(0.0, 0.0, 1.4), 0.5).unwrap();
        let wall = box_hull(Vec3::ZERO, Vec3::splat(1.0));

        let hit = contact(&ball, &wall).unwrap();
        // The ball is above the wall, so it parts downward, into it.
        assert!(faces(&hit, -Vec3::Z), "normal {:?}", hit.normal);
        assert!((hit.depth - 0.1).abs() < 1e-4, "depth {}", hit.depth);
        assert!(separates(&ball, &wall, &hit));
    }

    #[test]
    fn a_ball_wedged_inside_a_box_leaves_by_the_nearest_face() {
        let ball = Solid::ball(Vec3::new(0.9, 0.0, 0.0), 0.5).unwrap();
        let wall = box_hull(Vec3::ZERO, Vec3::splat(1.0));

        let hit = contact(&ball, &wall).unwrap();
        // The nearest face is the one the ball is already heading out of, and it
        // is behind it, so the normal points back toward the box's middle.
        assert!(faces(&hit, -Vec3::X), "normal {:?}", hit.normal);
        // The near face is a tenth away, and the ball's own radius is half.
        assert!((hit.depth - 0.6).abs() < 1e-4, "depth {}", hit.depth);
        assert!(separates(&ball, &wall, &hit));
    }

    #[test]
    fn a_capsule_lying_over_a_box_is_pushed_off_it() {
        let box_top = 1.0;
        let capsule = Solid::capsule(
            [Vec3::new(0.0, 0.0, box_top + 0.3), Vec3::new(0.0, 0.0, 2.0)],
            0.5,
        )
        .unwrap();
        let box_ = box_hull(Vec3::ZERO, Vec3::splat(1.0));

        let hit = contact(&capsule, &box_).unwrap();
        assert!(faces(&hit, -Vec3::Z), "normal {:?}", hit.normal);
        assert!((hit.depth - 0.2).abs() < 1e-4, "depth {}", hit.depth);
        assert!(separates(&capsule, &box_, &hit));
    }

    #[test]
    fn crossing_capsules_have_no_face_and_part_by_their_middles() {
        let along_x =
            Solid::capsule([Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)], 0.3).unwrap();
        let along_y =
            Solid::capsule([Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)], 0.3).unwrap();

        // Their difference is a flat sheet, so no face of it can be trusted and
        // the middles take over. The answer must still be a real one.
        let hit = contact(&along_x, &along_y).unwrap();
        assert!(hit.depth > 0.0);
        assert!((hit.normal.length() - 1.0).abs() < 1e-4);
        assert!(separates(&along_x, &along_y, &hit));
    }

    #[test]
    fn a_hull_of_triangles_meets_a_box_like_anything_else() {
        let pyramid = Solid::new(
            vec![
                Vec3::new(1.4, 0.0, 1.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, -1.0, 0.0),
                Vec3::new(-1.0, 0.0, 2.0),
            ],
            0.0,
        )
        .unwrap();
        let wall = box_hull(Vec3::ZERO, Vec3::splat(1.0));

        assert!(overlaps(&pyramid, &wall));
        let hit = contact(&pyramid, &wall).unwrap();
        assert!(hit.depth > 0.0);
        assert!(separates(&pyramid, &wall, &hit));
    }

    #[test]
    fn shapes_that_only_touch_are_not_pushed() {
        let a = box_hull(Vec3::ZERO, Vec3::splat(1.0));
        let b = box_hull(Vec3::new(2.0, 0.0, 0.0), Vec3::splat(1.0));

        assert!(
            contact(&a, &b).is_none_or(|hit| hit.depth <= 1e-4),
            "touching is not colliding: {:?}",
            contact(&a, &b)
        );
        assert!(!overlaps(&a, &b), "touching is not colliding");

        let balls = (
            Solid::ball(Vec3::ZERO, 1.0).unwrap(),
            Solid::ball(Vec3::new(2.0, 0.0, 0.0), 1.0).unwrap(),
        );
        assert!(!overlaps(&balls.0, &balls.1));
    }

    #[test]
    fn a_contact_that_does_not_clear_the_pair_is_dropped() {
        let a = Solid::ball(Vec3::ZERO, 1.0).unwrap();
        let b = Solid::ball(Vec3::new(1.0, 0.0, 0.0), 1.0).unwrap();

        assert!(separates(
            &a,
            &b,
            &Contact {
                normal: Vec3::X,
                depth: 1.0
            }
        ));
        assert!(
            !separates(
                &a,
                &b,
                &Contact {
                    normal: Vec3::Z,
                    depth: 0.001
                }
            ),
            "a suspect normal must not push"
        );
        assert!(!separates(
            &a,
            &b,
            &Contact {
                normal: Vec3::X,
                depth: -1.0
            }
        ));
        assert!(!separates(
            &a,
            &b,
            &Contact {
                normal: Vec3::ZERO,
                depth: 1.0
            }
        ));
    }

    #[test]
    fn the_broad_phase_refuses_a_walk_across_the_map() {
        let here = box_hull(Vec3::ZERO, Vec3::splat(1.0));
        let elsewhere = box_hull(Vec3::new(500.0, 500.0, 500.0), Vec3::splat(1.0));

        assert!(!Solid::in_boxes(&here, &elsewhere));
        assert_eq!(contact(&here, &elsewhere), None);
    }

    #[test]
    fn the_box_a_solid_sits_in_reaches_its_full_length() {
        let capsule = Solid::capsule([Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0)], 0.5).unwrap();
        let (min, max) = capsule.aabb();

        assert_eq!(min, Vec3::new(-0.5, -0.5, -0.5));
        assert_eq!(max, Vec3::new(0.5, 0.5, 2.5));
    }

    #[test]
    fn a_shape_that_is_nowhere_is_refused() {
        assert_eq!(Solid::new(vec![], 1.0), None);
        assert_eq!(
            Solid::new(vec![Vec3::new(f32::NAN, 0.0, 0.0)], 1.0),
            None,
            "a hull that is not a number is not a shape"
        );
        assert_eq!(Solid::ball(Vec3::ZERO, -1.0), None);
        assert_eq!(
            Solid::capsule([Vec3::ZERO, Vec3::ZERO], 0.0).map(|s| s.points.len()),
            Some(2)
        );
    }

    #[test]
    fn a_rotated_pair_still_parts_the_same_ways() {
        // A box stood on a corner against a box flat on a face: the walk sees
        // only points, so it answers the corner case like any other.
        let upright = box_hull(Vec3::new(1.4, 0.0, 0.0), Vec3::new(0.2, 0.2, 1.4));
        let flat = box_hull(Vec3::ZERO, Vec3::new(2.0, 2.0, 0.5));

        let hit = contact(&flat, &upright).unwrap();
        assert!(hit.depth > 0.0);
        assert!(separates(&flat, &upright, &hit));
        let back = contact(&upright, &flat).unwrap();
        assert!((hit.normal + back.normal).length() < 1e-3);
    }
}
