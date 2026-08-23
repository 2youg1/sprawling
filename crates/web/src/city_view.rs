// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The isometric canvas. Stage 4 builds the geometry
//! and the interface; the drawing layer arrives at P2 and this file's
//! signatures do not move when it does.
//!
//! **It is a canvas, not a thousand DOM nodes.** A thousand Residents given
//! to the document object model is a thousand elements whose style and
//! layout the browser must maintain, when the layer only ever needs "draw
//! shapes at coordinates". Drawing in Rust also lets the headless bitmap
//! regression share the exact code the browser runs, which is a stronger
//! check than a screenshot that depends on font rendering.
//!
//! **One geometry, two readers.** Hit testing and drawing use the same
//! functions here. Two implementations of one projection is two authorities,
//! and the one that drifts is always the one nobody is looking at.

use std::collections::BTreeSet;

use channels::{Address, BuildingProgress, CityAnswer, ClientFrame, Progress, Query};
use dioxus::prelude::*;

/// Logical tile, 2:1 axonometric.
pub const TILE_RATIO: u32 = 2;

/// How wide the city's grid is. Placement is a hash into this square, so
/// the extent decides how much room the hash has before two buildings
/// land on the same tile.
pub const CITY_EXTENT: u32 = 12;

/// Camera zoom stops. Three, because a continuous zoom invites a person to
/// hunt for the right level instead of reading the city.
pub const ZOOM_STOPS: [u32; 3] = [1, 2, 4];

/// Tile size in pixels, derived from the viewport and the city's extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    /// Tile width in pixels.
    pub tile_width: u32,
    /// Tile height: always half the width, by the 2:1 projection.
    pub tile_height: u32,
    /// The viewport this camera was fitted to, and the city extent it was
    /// fitted for. Both are kept because the city is *centred*: without
    /// them `project` has no idea where the middle is, and every tile
    /// whose `v` exceeds its `u` lands at a negative x. That is not
    /// hypothetical - it is why the city page was a blank canvas with one
    /// grey sliver in the top-left corner.
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub extent: u32,
    /// What a person panned, on top of the centred origin. Added when
    /// projecting and removed when unprojecting, in that one pair of
    /// methods, so panning cannot make the picture and the pick disagree.
    pub pan_x: i32,
    pub pan_y: i32,
}

impl Camera {
    /// Fits an `extent` by `extent` city into a viewport.
    ///
    /// `min(width / (2n+1), height / (n+3))`, then clamped. The content
    /// height uses `n * tile_width` rather than `n * tile_height`, because
    /// the extruded prisms stand above their tiles and a city sized to the
    /// flat diamond would clip its own towers.
    #[must_use]
    pub fn fit(viewport_width: u32, viewport_height: u32, extent: u32) -> Self {
        let horizontal_tiles = extent.saturating_mul(2).saturating_add(1);
        let vertical_tiles = extent.saturating_add(3);
        let by_width = viewport_width
            .checked_div(horizontal_tiles.max(1))
            .unwrap_or_default();
        let by_height = viewport_height
            .checked_div(vertical_tiles.max(1))
            .unwrap_or_default();
        // Down to a multiple of four. Two halvings happen on the way from a
        // tile to a pixel - width to height, then each to its half - so an
        // odd width makes `tile_height * 2 != tile_width` and hit testing
        // stops inverting drawing. Found by the 2:1 assertion on a 9-pixel
        // fit; the alternative was to carry the error and let picks drift.
        let fitted = by_width.min(by_height).clamp(8, 128);
        let tile_width = fitted.checked_div(4).unwrap_or(2).max(2).saturating_mul(4);
        Self {
            tile_width,
            tile_height: tile_width
                .checked_div(TILE_RATIO)
                .unwrap_or(tile_width)
                .max(1),
            viewport_width,
            viewport_height,
            extent: extent.max(1),
            pan_x: 0,
            pan_y: 0,
        }
    }

    /// Where tile `(0, 0)` sits, before any panning.
    ///
    /// Horizontally the middle, because the diamond grows both ways from
    /// there. Vertically high enough that the whole diamond is on screen
    /// and the towers standing on its near edge still have air above them.
    #[must_use]
    pub fn origin(&self) -> (i32, i32) {
        let half_viewport =
            i32::try_from(self.viewport_width.checked_div(2).unwrap_or(0)).unwrap_or_default();
        let diamond = i32::try_from(
            self.extent
                .saturating_sub(1)
                .saturating_mul(self.tile_height),
        )
        .unwrap_or_default();
        let middle =
            i32::try_from(self.viewport_height.checked_div(2).unwrap_or(0)).unwrap_or_default();
        let headroom = i32::try_from(self.tile_height).unwrap_or_default();
        (
            half_viewport,
            middle
                .saturating_sub(diamond.checked_div(2).unwrap_or_default())
                .saturating_add(headroom),
        )
    }

    /// The same camera at one of the three stops.
    ///
    /// Stops rather than a continuous zoom, because a slider invites a
    /// person to hunt for the right level instead of reading the city.
    /// An index past the end takes the last stop: a control that cannot
    /// go further should stop, not wrap around to the smallest view.
    #[must_use]
    pub fn at_stop(self, stop: usize) -> Self {
        let factor = ZOOM_STOPS
            .get(stop)
            .copied()
            .unwrap_or_else(|| ZOOM_STOPS.last().copied().unwrap_or(1));
        let tile_width = self.tile_width.saturating_mul(factor);
        Self {
            tile_width,
            // Recomputed rather than scaled, so the 2:1 ratio holds at
            // every stop and hit testing keeps inverting drawing.
            tile_height: tile_width
                .checked_div(TILE_RATIO)
                .unwrap_or(tile_width)
                .max(1),
            ..self
        }
    }

    /// The same camera moved by a pixel offset.
    #[must_use]
    pub fn panned_by(self, dx: i32, dy: i32) -> Self {
        Self {
            pan_x: self.pan_x.saturating_add(dx),
            pan_y: self.pan_y.saturating_add(dy),
            ..self
        }
    }

    /// Projects a tile coordinate to a pixel position.
    #[must_use]
    pub fn project(&self, u: i32, v: i32) -> (i32, i32) {
        let half_width = i32::try_from(self.tile_width.checked_div(2).unwrap_or(1)).unwrap_or(1);
        let half_height = i32::try_from(self.tile_height.checked_div(2).unwrap_or(1)).unwrap_or(1);
        let (origin_x, origin_y) = self.origin();
        let x = u
            .saturating_sub(v)
            .saturating_mul(half_width)
            .saturating_add(origin_x)
            .saturating_add(self.pan_x);
        let y = u
            .saturating_add(v)
            .saturating_mul(half_height)
            .saturating_add(origin_y)
            .saturating_add(self.pan_y);
        (x, y)
    }

    /// Inverts [`Camera::project`]. The same halves, read backwards: hit
    /// testing cannot drift from drawing because it is not a second
    /// derivation.
    #[must_use]
    pub fn unproject(&self, x: i32, y: i32) -> (i32, i32) {
        let half_width = i32::try_from(self.tile_width.checked_div(2).unwrap_or(1)).unwrap_or(1);
        let half_height = i32::try_from(self.tile_height.checked_div(2).unwrap_or(1)).unwrap_or(1);
        let (origin_x, origin_y) = self.origin();
        let x = x.saturating_sub(origin_x).saturating_sub(self.pan_x);
        let y = y.saturating_sub(origin_y).saturating_sub(self.pan_y);
        let a = x.checked_div(half_width).unwrap_or_default();
        let b = y.checked_div(half_height).unwrap_or_default();
        let u = a.saturating_add(b).checked_div(2).unwrap_or_default();
        let v = b.saturating_sub(a).checked_div(2).unwrap_or_default();
        (u, v)
    }
}

/// A Building's place on the grid, and how tall it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prism {
    pub id: String,
    pub u: i32,
    pub v: i32,
    /// Storeys: the logarithmic order of the Building's Asset count, so a
    /// city with one huge Building is still readable.
    pub storeys: u32,
    pub active: bool,
    /// What the tower says about itself under its own footprint - the
    /// plan's own numbers. Annotated directly rather than through a
    /// legend, which would ask a reader to hold a mapping in their head
    /// while looking somewhere else.
    pub note: String,
}

/// Places a Building deterministically from its id.
///
/// A stable hash rather than insertion order: the same city state must
/// render the same picture every time, or a bitmap comparison is worthless
/// and a person loses the spatial memory the whole metaphor is for.
#[must_use]
pub fn place(id: &str, extent: u32) -> (i32, i32) {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let span = u64::from(extent.max(1));
    let u = hash.checked_rem(span).unwrap_or_default();
    let v = hash
        .checked_div(span)
        .and_then(|rest| rest.checked_rem(span))
        .unwrap_or_default();
    (
        i32::try_from(u).unwrap_or_default(),
        i32::try_from(v).unwrap_or_default(),
    )
}

/// Storeys from an Asset count: the logarithmic order, floor of log2 plus
/// one. Linear height would make one large Building dwarf the city into
/// invisibility.
#[must_use]
pub fn storeys(assets: u64) -> u32 {
    if assets == 0 {
        return 1;
    }
    let bits = u64::BITS.saturating_sub(assets.leading_zeros());
    bits.max(1)
}

/// Orders prisms for the painter: ground, then Residents, then Buildings by
/// `u + v` ascending, so nearer prisms are drawn over farther ones.
#[must_use]
pub fn painter_order(mut prisms: Vec<Prism>) -> Vec<Prism> {
    prisms.sort_by_key(|prism| (prism.u.saturating_add(prism.v), prism.u, prism.id.clone()));
    prisms
}

/// The three faces' tokens. Form stands up on lightness difference, not on
/// outlines - an outlined box reads as a diagram, a lit box reads as a
/// solid.
#[must_use]
pub fn face_tokens(active: bool, selected: bool) -> (&'static str, &'static str, &'static str) {
    let top = if selected {
        "G7"
    } else if active {
        "G6"
    } else {
        "G5"
    };
    (top, "G4", "G2")
}

/// The prisms of a city as the server described it.
///
/// Two sources, on purpose. Where a building stands and how tall it is
/// come from the plan it published, which changes when someone writes a
/// roadmap. Whether it is lit comes from the runs in flight, which the
/// event stream already folds. Asking the server again on every event
/// would turn a fold into a poll.
#[must_use]
pub fn prisms_of(buildings: &[BuildingProgress], busy: &BTreeSet<Address>) -> Vec<Prism> {
    let mut prisms: Vec<Prism> = buildings
        .iter()
        .map(|building| {
            let (u, v) = place(building.addr.as_str(), CITY_EXTENT);
            Prism {
                id: building.addr.as_str().to_owned(),
                u,
                v,
                storeys: storeys(scale_of(building.progress)),
                active: busy.contains(&building.addr),
                // The words for a plan's progress come from the one
                // module that writes them, so the
                // label under a tower and the bar on a building's page
                // cannot drift apart.
                note: crate::progress::bar(
                    &building.progress,
                    false,
                    crate::progress::Subject::Plan,
                )
                .label,
            }
        })
        .collect();
    // The hash spreads buildings over a twelve-by-twelve square; the view
    // shows the part that is occupied. Re-basing to the corner of that
    // part is a translation, so it keeps the placement deterministic and
    // keeps drawing and picking reading the same coordinates.
    let low_u = prisms.iter().map(|prism| prism.u).min().unwrap_or_default();
    let low_v = prisms.iter().map(|prism| prism.v).min().unwrap_or_default();
    for prism in &mut prisms {
        prism.u = prism.u.saturating_sub(low_u);
        prism.v = prism.v.saturating_sub(low_v);
    }
    prisms
}

/// A building's size is the work it has taken on, not the work it has
/// finished: a building that just finished everything does not shrink.
/// Without a denominator there is no size to read, so the building is one
/// storey — the same refusal to invent a number that `Progress` makes in
/// the type.
fn scale_of(progress: Progress) -> u64 {
    match progress {
        Progress::Planned(planned) => u64::from(planned.ratio().1),
        Progress::Unplanned(_) => 0,
    }
}

/// The rows a building's plan could not state. Shown rather than dropped:
/// a plan quietly missing two lines is worse than no plan, because it
/// reads as complete.
#[must_use]
pub fn unreadable_rows(buildings: &[BuildingProgress]) -> Vec<String> {
    let mut rows = Vec::new();
    for building in buildings {
        for problem in &building.problems {
            rows.push(format!("{}: {problem}", building.addr.as_str()));
        }
    }
    rows
}

/// One filled shape and the token that colours it. Four points because
/// every face of a prism is a quadrilateral: the top diamond and the two
/// visible sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    pub id: String,
    pub token: &'static str,
    pub points: [(i32, i32); 4],
}

/// A word drawn on the city: which building it belongs to, where it sits,
/// and how loud it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub id: String,
    pub at: (i32, i32),
    pub text: String,
    pub token: &'static str,
    /// Whether it is the building's name rather than its numbers.
    pub leading: bool,
}

/// What to paint, in the order to paint it. A list of shapes rather than
/// a sequence of canvas calls: the browser turns it into calls, and a
/// headless run turns the same list into a bitmap, so the two cannot
/// drift apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayList {
    pub camera: Camera,
    /// The plate the city stands on, and its tiles. Its own field rather
    /// than the first faces, because everything in `faces` belongs to a
    /// building and is answerable to a click; the ground belongs to
    /// nobody.
    pub ground: Vec<Face>,
    pub faces: Vec<Face>,
    /// The outline of the selected building's top face, if one is picked.
    /// Stroked rather than filled, and the only stroke in the picture.
    pub outline: Option<[(i32, i32); 4]>,
    pub labels: Vec<Label>,
}

/// How much a storey lifts a prism: half a tile height, so a building of
/// three storeys is visibly taller than one of two at every zoom stop
/// without the tower leaving the viewport the camera fitted.
fn storey_lift(camera: &Camera) -> i32 {
    i32::try_from(camera.tile_height.checked_div(2).unwrap_or(1))
        .unwrap_or(1)
        .max(1)
}

/// The three visible faces of one prism, top first.
///
/// This is the only place a prism becomes geometry. Drawing calls it and
/// so does picking, which is what makes "the pick follows the picture" a
/// property rather than a promise.
#[must_use]
pub fn faces_of(camera: &Camera, prism: &Prism, selected: bool) -> [Face; 3] {
    let (cx, cy) = camera.project(prism.u, prism.v);
    let half_width = i32::try_from(camera.tile_width.checked_div(2).unwrap_or(1)).unwrap_or(1);
    let half_height = i32::try_from(camera.tile_height.checked_div(2).unwrap_or(1)).unwrap_or(1);
    let lift = storey_lift(camera).saturating_mul(i32::try_from(prism.storeys).unwrap_or(1));

    // Ground diamond, then the same diamond lifted by the building's height.
    let north = (cx, cy.saturating_sub(half_height).saturating_sub(lift));
    let east = (cx.saturating_add(half_width), cy.saturating_sub(lift));
    let south = (cx, cy.saturating_add(half_height).saturating_sub(lift));
    let west = (cx.saturating_sub(half_width), cy.saturating_sub(lift));
    let ground_south = (south.0, south.1.saturating_add(lift));
    let ground_west = (west.0, west.1.saturating_add(lift));
    let ground_east = (east.0, east.1.saturating_add(lift));

    let (top, left, right) = face_tokens(prism.active, selected);
    [
        Face {
            id: prism.id.clone(),
            token: top,
            points: [north, east, south, west],
        },
        Face {
            id: prism.id.clone(),
            token: left,
            points: [west, south, ground_south, ground_west],
        },
        Face {
            id: prism.id.clone(),
            token: right,
            points: [south, east, ground_east, ground_south],
        },
    ]
}

/// Turns a city into the shapes that draw it.
///
/// The order is the painter's: farther prisms first, so nearer ones cover
/// them. Within a prism the top face comes first, because the sides hang
/// below it and nothing of the same prism can occlude it.
#[must_use]
pub fn draw(camera: &Camera, prisms: Vec<Prism>, selected: Option<&str>) -> DisplayList {
    let mut faces = Vec::new();
    let mut labels = Vec::new();
    let mut outline = None;
    for prism in painter_order(prisms) {
        let is_selected = selected.is_some_and(|id| id == prism.id);
        let sides = faces_of(camera, &prism, is_selected);
        if is_selected {
            outline = sides.first().map(|top| top.points);
        }
        faces.extend(sides);
        faces.extend(windows_of(camera, &prism));
        labels.extend(labels_of(camera, &prism));
    }
    DisplayList {
        camera: *camera,
        ground: ground_of(camera),
        faces,
        outline,
        labels,
    }
}

/// The ground the city stands on: the diamond of the whole extent, one
/// step lighter than the page behind it. Drawn first, so a building at
/// the far corner still reads as standing *on* something.
#[must_use]
pub fn ground_of(camera: &Camera) -> Vec<Face> {
    let last = i32::try_from(camera.extent.saturating_sub(1)).unwrap_or_default();
    let plate = Face {
        id: String::new(),
        token: "G1",
        points: [
            camera.project(0, 0),
            camera.project(last, 0),
            camera.project(last, last),
            camera.project(0, last),
        ],
    };
    let mut ground = vec![plate];
    // A tile pattern, so distance reads. Two lightnesses one step apart:
    // enough to see the grid, not enough to compete with a building.
    for u in 0..=last {
        for v in 0..=last {
            if u.saturating_add(v).checked_rem(2) != Some(0) {
                continue;
            }
            let (cx, cy) = camera.project(u, v);
            let half_width =
                i32::try_from(camera.tile_width.checked_div(2).unwrap_or(1)).unwrap_or(1);
            let half_height =
                i32::try_from(camera.tile_height.checked_div(2).unwrap_or(1)).unwrap_or(1);
            ground.push(Face {
                id: String::new(),
                token: "G2",
                points: [
                    (cx, cy.saturating_sub(half_height)),
                    (cx.saturating_add(half_width), cy),
                    (cx, cy.saturating_add(half_height)),
                    (cx.saturating_sub(half_width), cy),
                ],
            });
        }
    }
    ground
}

/// How wide a grid the placed buildings actually occupy.
///
/// The camera fits *this* rather than the hash's whole square: two
/// buildings in a twelve-by-twelve grid are two specks in an empty field,
/// and a city view whose subject is too small to read is a decoration.
#[must_use]
pub fn occupied_extent(prisms: &[Prism]) -> u32 {
    let span = |values: Vec<i32>| -> u32 {
        let low = values.iter().copied().min().unwrap_or_default();
        let high = values.iter().copied().max().unwrap_or_default();
        u32::try_from(high.saturating_sub(low).saturating_add(1)).unwrap_or(1)
    };
    let us = span(prisms.iter().map(|prism| prism.u).collect());
    let vs = span(prisms.iter().map(|prism| prism.v).collect());
    us.max(vs).saturating_add(1).clamp(3, CITY_EXTENT)
}

/// A point `num/den` of the way from `a` to `b`, dropped by `fall`
/// pixels. Integer throughout: the picture must be the same on every
/// machine that draws it.
fn along(a: (i32, i32), b: (i32, i32), num: i32, den: i32, fall: i32) -> (i32, i32) {
    let step = |from: i32, to: i32| {
        to.saturating_sub(from)
            .saturating_mul(num)
            .checked_div(den.max(1))
            .unwrap_or_default()
            .saturating_add(from)
    };
    (step(a.0, b.0), step(a.1, b.1).saturating_add(fall))
}

/// The windows of one tower.
///
/// Unlit windows are drawn too: "lit" only means
/// something where there is an unlit one beside it. A building with work
/// in flight lights one window per storey, which is the only place
/// activity is said in colour rather than in lightness.
#[must_use]
pub fn windows_of(camera: &Camera, prism: &Prism) -> Vec<Face> {
    let unit = storey_lift(camera);
    if unit < 6 {
        // Below this a window is a smudge, and a smudge is noise.
        return Vec::new();
    }
    let [top, _, _] = faces_of(camera, prism, false);
    let (Some(west), Some(south), Some(east)) =
        (top.points.first(), top.points.get(2), top.points.get(1))
    else {
        return Vec::new();
    };
    // The two visible walls hang from the top diamond's near edges.
    let walls = [(*west, *south), (*south, *east)];
    let mut windows = Vec::new();
    for storey in 0..prism.storeys {
        let drop = unit.saturating_mul(i32::try_from(storey).unwrap_or_default());
        let head = drop.saturating_add(unit.checked_div(4).unwrap_or(1));
        let foot = drop.saturating_add(unit.saturating_mul(3).checked_div(4).unwrap_or(1));
        for (wall, (from, to)) in walls.iter().enumerate() {
            for slot in 0i32..2 {
                let (near, far) = (
                    slot.saturating_mul(4).saturating_add(1),
                    slot.saturating_mul(4).saturating_add(3),
                );
                let lit =
                    prism.active && storey.checked_rem(2) == Some(0) && wall == 0 && slot == 1;
                windows.push(Face {
                    id: prism.id.clone(),
                    token: if lit { "G9" } else { "G3" },
                    points: [
                        along(*from, *to, near, 8, head),
                        along(*from, *to, far, 8, head),
                        along(*from, *to, far, 8, foot),
                        along(*from, *to, near, 8, foot),
                    ],
                });
            }
        }
    }
    windows
}

/// A tower's two lines: its name, and what its own plan says about it.
#[must_use]
pub fn labels_of(camera: &Camera, prism: &Prism) -> Vec<Label> {
    let (cx, cy) = camera.project(prism.u, prism.v);
    let below = cy
        .saturating_add(i32::try_from(camera.tile_height).unwrap_or_default())
        .saturating_add(4);
    vec![
        Label {
            id: prism.id.clone(),
            at: (cx, below),
            text: prism.id.clone(),
            token: if prism.active { "G10" } else { "G8" },
            leading: true,
        },
        Label {
            id: prism.id.clone(),
            at: (cx, below.saturating_add(14)),
            text: prism.note.clone(),
            token: "G6",
            leading: false,
        },
    ]
}

/// Which building is under a pixel, or none.
///
/// Reads the same faces `draw` emits, in reverse: the last prism painted
/// is the one on top, so it is the one a person means when they click
/// where two overlap.
#[must_use]
pub fn pick(camera: &Camera, prisms: Vec<Prism>, x: i32, y: i32) -> Option<String> {
    for prism in painter_order(prisms).iter().rev() {
        if faces_of(camera, prism, false)
            .iter()
            .any(|face| contains(&face.points, x, y))
        {
            return Some(prism.id.clone());
        }
    }
    None
}

/// Point in convex quadrilateral, in integers. Every cross product has
/// the same sign inside; a zero means the point is on an edge, which
/// counts as inside so that two touching faces never leave a seam a click
/// can fall through.
fn contains(points: &[(i32, i32); 4], x: i32, y: i32) -> bool {
    let mut positive = false;
    let mut negative = false;
    for (index, (ax, ay)) in points.iter().copied().enumerate() {
        let next = index
            .saturating_add(1)
            .checked_rem(points.len())
            .unwrap_or(0);
        let Some((bx, by)) = points.get(next).copied() else {
            continue;
        };
        let cross = i64::from(bx.saturating_sub(ax))
            .saturating_mul(i64::from(y.saturating_sub(ay)))
            .saturating_sub(
                i64::from(by.saturating_sub(ay)).saturating_mul(i64::from(x.saturating_sub(ax))),
            );
        if cross > 0 {
            positive = true;
        }
        if cross < 0 {
            negative = true;
        }
    }
    !(positive && negative)
}

/// Turns one display list into canvas calls, and decides nothing.
///
/// The humble half of the pair: every question about where a face is or
/// what colour it takes was answered in the pure functions above, so the
/// browser-only code has no branch a test would want to reach.
#[cfg(target_arch = "wasm32")]
pub fn paint(canvas: &web_sys::HtmlCanvasElement, list: &DisplayList) -> Option<()> {
    use wasm_bindgen::JsCast;

    let context = canvas
        .get_context("2d")
        .ok()
        .flatten()?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .ok()?;
    let width = f64::from(canvas.width());
    let height = f64::from(canvas.height());
    context.clear_rect(0.0, 0.0, width, height);
    if let Some(backdrop) = crate::theme::gray_colour("G0") {
        context.set_fill_style_str(&backdrop);
        context.fill_rect(0.0, 0.0, width, height);
    }
    for face in list.ground.iter().chain(list.faces.iter()) {
        let Some(colour) = crate::theme::gray_colour(face.token) else {
            continue;
        };
        context.set_fill_style_str(&colour);
        context.begin_path();
        for (index, (x, y)) in face.points.iter().enumerate() {
            let (x, y) = (f64::from(*x), f64::from(*y));
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        context.close_path();
        context.fill();
    }
    // The only stroke in the picture, and the only accent: what a person
    // has selected. Form is carried by lightness everywhere else.
    if let Some(points) = list.outline {
        context.set_stroke_style_str("var(--ACCENT)");
        if let Some(accent) = crate::theme::gray_colour("G10") {
            context.set_stroke_style_str(&accent);
        }
        context.set_line_width(2.0);
        context.begin_path();
        for (index, (x, y)) in points.iter().enumerate() {
            let (x, y) = (f64::from(*x), f64::from(*y));
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        context.close_path();
        context.stroke();
    }
    context.set_text_align("center");
    for label in &list.labels {
        let Some(colour) = crate::theme::gray_colour(label.token) else {
            continue;
        };
        context.set_fill_style_str(&colour);
        context.set_font(if label.leading {
            "600 13px 'Noto Sans SC', system-ui, sans-serif"
        } else {
            "12px 'Noto Sans SC', system-ui, sans-serif"
        });
        let _ = context.fill_text(&label.text, f64::from(label.at.0), f64::from(label.at.1));
    }
    Some(())
}

/// What a person typed into the building form, and whether it is a
/// command yet.
///
/// A building is a top-level address, so an address with a slash in it
/// is refused here as well as at the server — shown before the person
/// presses anything rather than after.
#[must_use]
pub fn create_command(addr: &str, template: &str) -> Option<ClientFrame> {
    let addr = Address::parse(addr.trim()).ok()?;
    if addr.as_str().contains('/') {
        return None;
    }
    let template = channels::TemplateName::parse(template.trim()).ok()?;
    Some(ClientFrame::Command(Box::new(
        channels::WireCommand::CreateBuilding {
            idem: channels::IdemKey::derive(
                &channels::RunId::CITY,
                channels::Seq::FIRST,
                addr.as_str().as_bytes(),
            ),
            addr,
            template,
        },
    )))
}

/// What a person typed into the selected building's form, and whether it
/// is a dispatch yet.
///
/// A run needs somewhere to work and something that counts as done, so
/// both are required here rather than defaulted: a dispatch with an
/// invented goal is a run that cannot report it finished.
#[must_use]
pub fn dispatch_command(building: &str, task: &str, goal: &str) -> Option<ClientFrame> {
    // Work happens in a room, not at a building's root: living there
    // would hand a run the whole building's write domain. What a Dispatch
    // frame looks like is `app::dispatch_command`'s answer and only its
    // answer - this page decides the address, not the shape.
    crate::app::dispatch_command(&format!("{}/room1", building.trim()), task, goal, "plan")
}

/// How far one press of a pan control moves the city, in pixels. A whole
/// step rather than a smooth glide: the view is being read, not flown
/// through, and an animation here would ask for attention the page has
/// no reason to take.
pub const PAN_STEP: i32 = 64;

/// The city page.
///
/// It asks for the city once when it mounts, because buildings appear
/// when someone creates one and not on every event; what moves with the
/// event stream is which of them are lit, and that arrives through
/// `busy` without another question.
#[component]
pub fn CityView(
    city: Option<CityAnswer>,
    busy: BTreeSet<Address>,
    selected: Option<String>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
    on_select: EventHandler<Option<String>>,
    /// The way into a building's own pages. The nav cannot carry them -
    /// a city may hold fifty buildings - so the city is the way in.
    on_open: EventHandler<String>,
) -> Element {
    let asked = use_signal(|| false);
    let mut raising = use_signal(String::new);
    let mut template = use_signal(|| "minimal".to_owned());
    let mut stop = use_signal(|| 0usize);
    let mut pan = use_signal(|| (0i32, 0i32));
    let mut task = use_signal(String::new);
    let mut goal = use_signal(String::new);
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(Query::CityView));
        }
    });
    let Some(city) = city else {
        return rsx! {
            section { class: "city-view",
                crate::panel::Empty {
                    status: "asking the city what it holds".to_owned(),
                    what: "its buildings, how much work each has taken on, and which of them are busy right now"
                        .to_owned(),
                }
            }
        };
    };
    let prisms = prisms_of(&city.buildings, &busy);
    let listing: Vec<(String, String)> = prisms
        .iter()
        .map(|prism| (prism.id.clone(), prism.note.clone()))
        .collect();
    let (dx, dy) = *pan.read();
    let camera = Camera::fit(CANVAS_WIDTH, CANVAS_HEIGHT, occupied_extent(&prisms))
        .at_stop(*stop.read())
        .panned_by(dx, dy);
    let problems = unreadable_rows(&city.buildings);
    // The selected building's name, held twice outside the markup: the
    // submit closure keeps one for the length of the page, and the
    // disabled check reads another on every render. Empty when nothing
    // is selected, which is the case where the panel is not drawn.
    let submitting = selected.clone().unwrap_or_default();
    let checking = submitting.clone();
    let picking = prisms.clone();
    #[cfg(target_arch = "wasm32")]
    {
        let list = draw(&camera, prisms.clone(), selected.as_deref());
        // `use_reactive` because the display list is built from props,
        // and a plain effect closure captures the props of the *first*
        // render only. That is why the canvas showed the ground and no
        // buildings: it was painted once, before the city answered, and
        // never again.
        use_effect(use_reactive!(|(list,)| paint_mounted(&list)));
    }
    let raised = city.buildings.len();
    let busy_now = city.active;
    rsx! {
        section { class: "city-view",
            crate::panel::Panel {
                title: if raised == 0 { "this city has no buildings yet".to_owned() }
                    else { format!("{raised} building(s), {busy_now} run(s) in flight") },
                figure: "{raised}",
                scope: "a tower's height is the work its plan has taken on, not the work it has finished; a lit window is a run in flight right now"
                    .to_owned(),
                source: "where the buildings stand comes from one query, asked when this page opened; which of them are lit is folded from the event stream, record by record, and is never polled"
                    .to_owned(),
            canvas {
                id: CANVAS_ID,
                width: "{CANVAS_WIDTH}",
                height: "{CANVAS_HEIGHT}",
                onclick: move |event| {
                    let point = event.data().element_coordinates();
                    let hit = pick(
                        &camera,
                        picking.clone(),
                        canvas_pixel(point.x, CANVAS_WIDTH),
                        canvas_pixel(point.y, CANVAS_HEIGHT),
                    );
                    on_select.call(hit);
                },
            }
            form {
                class: "new-building",
                onsubmit: move |event| {
                    event.prevent_default();
                    let (named, kind) = (raising.read().clone(), template.read().clone());
                    if let Some(frame) = create_command(&named, &kind) {
                        on_frame.call(frame);
                        raising.set(String::new());
                        // The city does not announce a new building on
                        // the event stream this page folds, so it is
                        // asked again rather than assumed.
                        on_frame.call(ClientFrame::Query(Query::CityView));
                    }
                },
                input {
                    name: "addr",
                    placeholder: "a name for the building",
                    value: "{raising}",
                    oninput: move |event| raising.set(event.value()),
                }
                select {
                    name: "template",
                    onchange: move |event| template.set(event.value()),
                    option { value: "minimal", "minimal" }
                    option { value: "confidential", "confidential" }
                }
                button {
                    r#type: "submit",
                    disabled: create_command(&raising.read(), &template.read()).is_none(),
                    "raise a building"
                }
            }
            if city.buildings.is_empty() {
                crate::panel::Empty {
                    status: "this city has no buildings yet".to_owned(),
                    what: "a building is one line of business: its own rules, its own plan, its own archive, and the rooms work happens in. Raise one above and it appears here with the ground under it."
                        .to_owned(),
                }
            }
            // The index beside the picture. The canvas answers "where",
            // and a pixel hunt is no way to answer "which": this list is
            // how a building is selected without a mouse, and the only
            // route to its own pages that a keyboard can take.
            div { class: "index",
                for row in listing.clone() {
                    div { key: "{row.0}", class: "index-row",
                        button {
                            class: "pick",
                            "aria-current": if selected.as_deref() == Some(row.0.as_str()) { "true" } else { "false" },
                            onclick: {
                                let name = row.0.clone();
                                move |_| on_select.call(Some(name.clone()))
                            },
                            "{row.0}"
                        }
                        span { class: "note", "{row.1}" }
                        button {
                            class: "read",
                            onclick: {
                                let name = row.0.clone();
                                move |_| on_open.call(name.clone())
                            },
                            "read it"
                        }
                    }
                }
            }
            div { class: "camera",
                for (index , factor) in ZOOM_STOPS.iter().enumerate() {
                    button {
                        key: "{factor}",
                        r#type: "button",
                        // The current stop is said, not only shown: a
                        // control whose state is a shade of grey is a
                        // control a screen reader cannot report.
                        "aria-pressed": if *stop.read() == index { "true" } else { "false" },
                        onclick: move |_| stop.set(index),
                        "{factor}x"
                    }
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx.saturating_add(PAN_STEP), dy)),
                    "move left"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx.saturating_sub(PAN_STEP), dy)),
                    "move right"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx, dy.saturating_add(PAN_STEP))),
                    "move up"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx, dy.saturating_sub(PAN_STEP))),
                    "move down"
                }
                button {
                    r#type: "button",
                    disabled: *stop.read() == 0 && (dx, dy) == (0, 0),
                    onclick: move |_| {
                        stop.set(0);
                        pan.set((0, 0));
                    },
                    "fit"
                }
            }
            if let Some(id) = selected.clone() {
                div { class: "selected",
                    p { "{id}" }
                    button {
                        class: "open-building",
                        onclick: {
                            let name = id.clone();
                            move |_| on_open.call(name.clone())
                        },
                        "read what {id} has written down"
                    }
                    form {
                        class: "send-work",
                        onsubmit: move |event| {
                            event.prevent_default();
                            let frame = dispatch_command(&submitting, &task.read(), &goal.read());
                            if let Some(frame) = frame {
                                on_frame.call(frame);
                                task.set(String::new());
                                goal.set(String::new());
                            }
                        },
                        input {
                            name: "task",
                            placeholder: "what should happen here",
                            value: "{task}",
                            oninput: move |event| task.set(event.value()),
                        }
                        input {
                            name: "goal",
                            placeholder: "what counts as done",
                            value: "{goal}",
                            oninput: move |event| goal.set(event.value()),
                        }
                        button {
                            r#type: "submit",
                            disabled: dispatch_command(&checking, &task.read(), &goal.read()).is_none(),
                            "send work here"
                        }
                    }
                    button {
                        r#type: "button",
                        onclick: move |_| on_select.call(None),
                        "clear selection"
                    }
                }
            }
            if !problems.is_empty() {
                ul { class: "problems",
                    for row in problems {
                        li { key: "{row}", "{row}" }
                    }
                }
            }
            }
        }
    }
}

/// A browser pointer coordinate as a canvas pixel.
///
/// Pointer positions arrive as `f64` and there is no fallible conversion
/// to `i32` to lean on, so the value is clamped into the canvas first:
/// after the clamp the cast is total, and a click outside the canvas was
/// never going to hit a prism anyway. A `NaN` clamps to zero, which is
/// the top-left corner and hits nothing.
#[expect(
    clippy::as_conversions,
    reason = "f64 to i32 has no TryFrom; the value is clamped into the canvas first, and Rust's \
              float-to-int casts saturate, so this conversion is total"
)]
#[must_use]
fn canvas_pixel(value: f64, bound: u32) -> i32 {
    let ceiling = f64::from(bound);
    let bounded = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, ceiling)
    };
    bounded as i32
}

/// Canvas identity and size. Fixed rather than measured: a canvas that
/// resized itself would change the projection under the reader's spatial
/// memory, which is the one thing the metaphor is for.
const CANVAS_ID: &str = "city-canvas";
const CANVAS_WIDTH: u32 = 1000;
const CANVAS_HEIGHT: u32 = 560;

#[cfg(target_arch = "wasm32")]
fn paint_mounted(list: &DisplayList) {
    use wasm_bindgen::JsCast;

    let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(CANVAS_ID))
    else {
        return;
    };
    if let Ok(canvas) = element.dyn_into::<web_sys::HtmlCanvasElement>() {
        paint(&canvas, list);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;
    use channels::{PlannedProgress, UnplannedProgress};

    fn planned(addr: &str, done: u32, total: u32, problems: Vec<String>) -> BuildingProgress {
        BuildingProgress {
            addr: Address::parse(addr).unwrap(),
            progress: Progress::Planned(PlannedProgress {
                done,
                blocked: 0,
                total,
            }),
            problems,
        }
    }

    #[test]
    fn a_city_of_buildings_becomes_a_city_of_prisms() {
        let buildings = vec![
            planned("lab", 1, 8, Vec::new()),
            planned("mill", 0, 1, Vec::new()),
        ];
        let mut busy = BTreeSet::new();
        busy.insert(Address::parse("lab").unwrap());
        let prisms = prisms_of(&buildings, &busy);
        assert_eq!(prisms.len(), 2);
        let lab = prisms.iter().find(|p| p.id == "lab").unwrap();
        let mill = prisms.iter().find(|p| p.id == "mill").unwrap();
        assert!(lab.active, "a building with a run in it is lit");
        assert!(!mill.active);
        assert!(
            lab.storeys > mill.storeys,
            "height is the work taken on, and lab took on eight rows to mill's one"
        );
        assert_eq!(
            prisms_of(&buildings, &busy),
            prisms,
            "the same city places the same way twice"
        );
    }

    #[test]
    fn a_building_without_a_denominator_gets_no_invented_height() {
        let buildings = vec![BuildingProgress {
            addr: Address::parse("yard").unwrap(),
            progress: Progress::Unplanned(UnplannedProgress {
                steps: 40,
                budget: channels::BudgetUse::default(),
            }),
            problems: Vec::new(),
        }];
        let prisms = prisms_of(&buildings, &BTreeSet::new());
        assert_eq!(
            prisms[0].storeys, 1,
            "forty steps is not a size; a plan is, and there is none"
        );
    }

    #[test]
    fn a_plan_the_city_could_not_read_is_shown_rather_than_dropped() {
        let buildings = vec![planned(
            "lab",
            1,
            2,
            vec!["row 4 has three columns".to_owned()],
        )];
        let rows = unreadable_rows(&buildings);
        assert_eq!(rows, vec!["lab: row 4 has three columns".to_owned()]);
    }

    #[test]
    fn what_is_drawn_is_what_can_be_picked() {
        let buildings = vec![planned("lab", 1, 8, Vec::new())];
        let prisms = prisms_of(&buildings, &BTreeSet::new());
        let camera = Camera::fit(CANVAS_WIDTH, CANVAS_HEIGHT, CITY_EXTENT);
        let list = draw(&camera, prisms.clone(), None);
        let top = list.faces.first().expect("a building has faces");
        let (x, y) = top.points[0];
        let (bx, by) = top.points[2];
        let hit = pick(&camera, prisms, (x + bx) / 2, (y + by) / 2);
        assert_eq!(
            hit.as_deref(),
            Some("lab"),
            "the middle of a face belongs to the prism that face came from"
        );
    }

    #[test]
    fn sending_work_needs_a_room_a_task_and_a_definition_of_done() {
        assert!(dispatch_command("lab", "fix the timer", "the test passes").is_some());
        assert!(
            dispatch_command("lab", "  ", "the test passes").is_none(),
            "a run with nothing to do is not a command"
        );
        assert!(
            dispatch_command("lab", "fix the timer", "").is_none(),
            "a run with no definition of done cannot report that it is done"
        );
        assert!(
            dispatch_command("", "fix the timer", "the test passes").is_none(),
            "there is no building called nothing"
        );
    }

    #[test]
    fn work_is_sent_to_a_room_and_never_to_a_buildings_root() {
        let Some(ClientFrame::Command(command)) =
            dispatch_command("lab", "fix the timer", "the test passes")
        else {
            panic!("a complete form is a command");
        };
        let channels::WireCommand::Dispatch { addr, .. } = *command else {
            panic!("the send-work form makes a dispatch");
        };
        assert_eq!(
            addr.as_str(),
            "lab/room1",
            "a run at a building's root would hold the whole building's write domain"
        );
    }

    #[test]
    fn the_pick_follows_the_picture_at_every_stop_and_every_offset() {
        // The property the whole camera exists to keep: however the view
        // is zoomed or moved, clicking the middle of a drawn face names
        // the building that face came from. Projection and inversion are
        // one pair of methods, and this is the assertion that says so.
        let buildings = vec![
            planned("lab", 1, 8, Vec::new()),
            planned("mill", 2, 4, Vec::new()),
        ];
        let prisms = prisms_of(&buildings, &BTreeSet::new());
        for stop in 0..ZOOM_STOPS.len() {
            for (dx, dy) in [(0, 0), (37, -14), (-120, 60)] {
                let camera = Camera::fit(CANVAS_WIDTH, CANVAS_HEIGHT, CITY_EXTENT)
                    .at_stop(stop)
                    .panned_by(dx, dy);
                let list = draw(&camera, prisms.clone(), None);
                for face in &list.faces {
                    let (ax, ay) = face.points[0];
                    let (bx, by) = face.points[2];
                    let hit = pick(&camera, prisms.clone(), (ax + bx) / 2, (ay + by) / 2);
                    assert_eq!(
                        hit.as_deref(),
                        Some(face.id.as_str()),
                        "stop {stop}, pan ({dx}, {dy}): the picture and the pick disagree"
                    );
                }
            }
        }
    }

    #[test]
    fn a_stop_past_the_last_one_stops_rather_than_wrapping_to_the_smallest() {
        let base = Camera::fit(CANVAS_WIDTH, CANVAS_HEIGHT, CITY_EXTENT);
        let last = base.at_stop(ZOOM_STOPS.len() - 1);
        assert_eq!(base.at_stop(ZOOM_STOPS.len() + 5), last);
        assert!(last.tile_width > base.at_stop(0).tile_width);
    }

    #[test]
    fn a_tile_stays_twice_as_wide_as_it_is_tall_at_every_stop() {
        for stop in 0..ZOOM_STOPS.len() {
            let camera = Camera::fit(1920, 1080, 8).at_stop(stop);
            assert_eq!(
                camera.tile_width,
                camera.tile_height * 2,
                "stop {stop}: hit testing stops inverting drawing the moment this breaks"
            );
        }
    }

    #[test]
    fn panning_moves_the_city_and_leaves_its_shape_alone() {
        let camera = Camera::fit(CANVAS_WIDTH, CANVAS_HEIGHT, CITY_EXTENT);
        let moved = camera.panned_by(40, -25);
        assert_eq!(camera.project(3, 4).0 + 40, moved.project(3, 4).0);
        assert_eq!(camera.project(3, 4).1 - 25, moved.project(3, 4).1);
        assert_eq!(
            moved.unproject(moved.project(3, 4).0, moved.project(3, 4).1),
            (3, 4),
            "a round trip through a moved camera is still the identity"
        );
    }

    #[test]
    fn a_pointer_outside_the_canvas_lands_on_its_edge_and_never_off_it() {
        assert_eq!(canvas_pixel(12.7, CANVAS_WIDTH), 12);
        assert_eq!(canvas_pixel(-40.0, CANVAS_WIDTH), 0);
        assert_eq!(
            canvas_pixel(f64::from(u32::MAX), CANVAS_WIDTH),
            i32::try_from(CANVAS_WIDTH).unwrap()
        );
        assert_eq!(canvas_pixel(f64::NAN, CANVAS_HEIGHT), 0);
    }

    #[test]
    fn a_tile_is_always_twice_as_wide_as_it_is_tall() {
        for (width, height, extent) in [(1920, 1080, 8), (800, 600, 3), (320, 240, 16)] {
            let camera = Camera::fit(width, height, extent);
            assert_eq!(
                camera.tile_width,
                camera.tile_height * TILE_RATIO,
                "the projection is 2:1 at every fit"
            );
        }
    }

    #[test]
    fn a_tiny_viewport_still_yields_a_drawable_tile() {
        let camera = Camera::fit(1, 1, 1000);
        assert!(camera.tile_width >= 8, "clamped rather than zero");
        assert!(camera.tile_height >= 1);
    }

    #[test]
    fn every_fit_lands_on_a_multiple_of_four() {
        // Two halvings separate a tile from a pixel offset. Anything else
        // makes the projection lossy and hit testing stops inverting it.
        for width in [1, 7, 9, 101, 321, 1920, 4000] {
            for height in [1, 13, 99, 1080] {
                let camera = Camera::fit(width, height, 8);
                assert_eq!(
                    camera.tile_width % 4,
                    0,
                    "a {width}x{height} viewport produced {}",
                    camera.tile_width
                );
                assert_eq!(camera.tile_width, camera.tile_height * TILE_RATIO);
            }
        }
    }

    #[test]
    fn hit_testing_inverts_drawing_because_they_share_one_geometry() {
        let camera = Camera::fit(1920, 1080, 8);
        for (u, v) in [(0, 0), (3, 1), (7, 7), (2, 6)] {
            let (x, y) = camera.project(u, v);
            assert_eq!(
                camera.unproject(x, y),
                (u, v),
                "projecting then hit-testing must return the same tile"
            );
        }
    }

    #[test]
    fn placement_is_a_function_of_the_id_and_nothing_else() {
        // The same city state renders the same picture, or a bitmap
        // comparison is worthless and spatial memory never forms.
        assert_eq!(place("acme/floor1", 16), place("acme/floor1", 16));
        assert_ne!(place("acme/floor1", 16), place("acme/floor2", 16));
        let (u, v) = place("anything", 16);
        assert!((0..16).contains(&u) && (0..16).contains(&v));
    }

    #[test]
    fn height_is_logarithmic_so_one_giant_does_not_flatten_the_city() {
        assert_eq!(storeys(0), 1);
        assert_eq!(storeys(1), 1);
        assert_eq!(storeys(2), 2);
        assert_eq!(storeys(1024), 11);
        // A thousandfold difference in assets is an elevenfold difference in
        // height, not a thousandfold one.
        assert!(storeys(1_000_000) < storeys(1000) * 3);
    }

    #[test]
    fn nearer_prisms_are_painted_last() {
        let prism = |id: &str, u: i32, v: i32| Prism {
            id: id.to_owned(),
            u,
            v,
            storeys: 1,
            active: false,
            note: "1/2".to_owned(),
        };
        let ordered = painter_order(vec![
            prism("far", 0, 0),
            prism("near", 5, 5),
            prism("mid", 2, 1),
        ]);
        let names: Vec<&str> = ordered.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(names, ["far", "mid", "near"]);
    }

    #[test]
    fn painter_order_is_total_so_two_renders_agree() {
        let prism = |id: &str, u: i32, v: i32| Prism {
            id: id.to_owned(),
            u,
            v,
            storeys: 1,
            active: false,
            note: "1/2".to_owned(),
        };
        // Two prisms on the same depth line: the tiebreak must be total.
        let one = painter_order(vec![prism("b", 1, 2), prism("a", 1, 2)]);
        let other = painter_order(vec![prism("a", 1, 2), prism("b", 1, 2)]);
        assert_eq!(one, other);
    }

    #[test]
    fn faces_differ_in_lightness_so_form_stands_without_outlines() {
        let (top, left, right) = face_tokens(false, false);
        let faces: std::collections::BTreeSet<&str> = [top, left, right].into_iter().collect();
        assert_eq!(faces.len(), 3, "three faces, three lightnesses");
        assert_ne!(face_tokens(true, false).0, face_tokens(false, false).0);
        assert_ne!(face_tokens(false, true).0, face_tokens(true, false).0);
    }

    fn city() -> Vec<Prism> {
        ["lab", "vault", "mail"]
            .iter()
            .enumerate()
            .map(|(n, id)| {
                let (u, v) = place(id, 8);
                Prism {
                    id: (*id).to_owned(),
                    u,
                    v,
                    storeys: storeys(10u64.saturating_mul(u64::try_from(n).unwrap_or(0) + 1)),
                    active: n == 0,
                    note: "3/7".to_owned(),
                }
            })
            .collect()
    }

    #[test]
    fn a_fitted_city_is_inside_the_viewport_it_was_fitted_to() {
        // The defect this pins: `fit` computed a tile size and left the
        // origin at (0, 0), so every tile whose v exceeded its u projected
        // to a negative x and the page showed an empty canvas with one
        // grey sliver in the corner. Fitting means fitting.
        let camera = Camera::fit(CANVAS_WIDTH, CANVAS_HEIGHT, CITY_EXTENT);
        let last = i32::try_from(CITY_EXTENT - 1).unwrap();
        let tall = Prism {
            id: "lab".to_owned(),
            u: 0,
            v: 0,
            storeys: 8,
            active: true,
            note: "7/11".to_owned(),
        };
        let mut points = vec![];
        for (u, v) in [(0, 0), (last, 0), (0, last), (last, last)] {
            points.push(camera.project(u, v));
        }
        for face in faces_of(&camera, &tall, false) {
            points.extend(face.points);
        }
        for (x, y) in points {
            assert!(
                (0..=i32::try_from(CANVAS_WIDTH).unwrap()).contains(&x),
                "x {x} is outside the canvas"
            );
            assert!(
                (0..=i32::try_from(CANVAS_HEIGHT).unwrap()).contains(&y),
                "y {y} is outside the canvas"
            );
        }
    }

    #[test]
    fn the_same_city_draws_the_same_shapes_in_the_same_order() {
        let camera = Camera::fit(1920, 1080, 8);
        let first = draw(&camera, city(), None);
        let mut shuffled = city();
        shuffled.reverse();
        let second = draw(&camera, shuffled, None);
        assert_eq!(
            first, second,
            "the picture is a function of the city, not of the order the buildings arrived"
        );
        let sides = first
            .faces
            .iter()
            .filter(|face| face.token != "G3" && face.token != "G9")
            .count();
        assert_eq!(sides, 9, "three prisms, three faces each");
        assert!(
            first.labels.len() >= 6,
            "every tower says its name and what its plan says"
        );
    }

    #[test]
    fn a_click_lands_on_the_building_a_person_sees_there() {
        let camera = Camera::fit(1920, 1080, 8);
        for prism in city() {
            // The centre of a top face belongs to that prism and no other.
            let top = faces_of(&camera, &prism, false)[0].points;
            let x = (top[1].0 + top[3].0) / 2;
            let y = (top[0].1 + top[2].1) / 2;
            assert_eq!(
                pick(&camera, city(), x, y).as_deref(),
                Some(prism.id.as_str()),
                "picking reads the faces drawing wrote"
            );
        }
        // Far outside the city, nothing is picked rather than the nearest.
        assert_eq!(pick(&camera, city(), 100_000, 100_000), None);
    }

    #[test]
    fn where_two_buildings_overlap_the_one_in_front_is_picked() {
        let camera = Camera::fit(1920, 1080, 8);
        // Same tile column, one nearer: their silhouettes overlap.
        let behind = Prism {
            id: "behind".to_owned(),
            u: 2,
            v: 2,
            storeys: 4,
            active: false,
            note: "1/2".to_owned(),
        };
        let front = Prism {
            id: "front".to_owned(),
            u: 3,
            v: 3,
            storeys: 1,
            active: false,
            note: "1/2".to_owned(),
        };
        let prisms = vec![behind.clone(), front.clone()];
        let list = draw(&camera, prisms.clone(), None);
        let last = list.faces.last().unwrap();
        assert_eq!(last.id, "front", "the nearer prism is painted last");

        let top = faces_of(&camera, &front, false)[0].points;
        let x = (top[1].0 + top[3].0) / 2;
        let y = (top[0].1 + top[2].1) / 2;
        assert_eq!(pick(&camera, prisms, x, y).as_deref(), Some("front"));
    }

    #[test]
    fn a_taller_building_stands_higher_and_a_selected_one_is_lighter() {
        let camera = Camera::fit(1920, 1080, 8);
        let short = Prism {
            id: "a".to_owned(),
            u: 1,
            v: 1,
            storeys: 1,
            active: false,
            note: "1/2".to_owned(),
        };
        let tall = Prism {
            storeys: 5,
            ..short.clone()
        };
        let short_top = faces_of(&camera, &short, false)[0].points[0].1;
        let tall_top = faces_of(&camera, &tall, false)[0].points[0].1;
        assert!(tall_top < short_top, "more storeys reach further up");

        let plain = faces_of(&camera, &short, false)[0].token;
        let picked = faces_of(&camera, &short, true)[0].token;
        assert_ne!(plain, picked, "selection is visible without colour");
    }
}
