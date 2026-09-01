// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The isometric city, drawn as shapes in the document.
//!
//! **It was a canvas until F2.02, and the reason it was one did not reach
//! this picture.** The recorded argument was that a thousand Residents must
//! not become a thousand elements. This view has never drawn a Resident: it
//! draws Buildings, of which a city holds tens, and the canvas was charging
//! four certain costs for that hypothetical saving - a fixed bitmap resampled
//! by CSS on every display that is not exactly its size, no way to read a
//! custom property (which is why the selection outline settled for a grey
//! where the code plainly wanted `--ACCENT`), no hover, focus, or keyboard
//! reach without reimplementing all three, and a drawing path that existed
//! only on wasm and so was reachable by no host test or gate.
//!
//! **Hit testing is no longer a second derivation.** The browser tests hits
//! against the very polygons it painted, so "what is drawn is what can be
//! picked" stopped being an assertion and became the construction. The
//! inverse projection, the point-in-quadrilateral test and the pointer
//! coordinate clamp went with it.
//!
//! **The picture fills what it is given.** The viewBox is the bounding box
//! of what was drawn, so a city of three buildings is a picture of three
//! buildings rather than three specks in a fixed 1000x560 field. The old
//! fit reserved `2n+1` tile widths for a diamond `n` tiles wide, which is
//! where most of that empty field came from.
//!
//! **The silhouette is the data.** A tower's height is the work its plan
//! took on and the lit band up its walls is the part that is done, so
//! progress is read off the skyline rather than from a number beside it.

use std::collections::BTreeSet;

use crate::lang::{Msg, fill, say};
use channels::{Address, BuildingProgress, CityAnswer, ClientFrame, Progress, Query};
use dioxus::prelude::*;

/// Logical tile, 2:1 axonometric.
pub const TILE_RATIO: u32 = 2;

/// One tile, in user units.
///
/// A constant rather than a fit, because the viewBox is fitted to the
/// drawing instead of the drawing to a viewport - so this number sets the
/// *proportion* of a tile to a label and nothing else. A multiple of four:
/// two halvings happen between a tile and a point, and an odd width breaks
/// the 2:1 ratio the projection is built on.
pub const TILE_WIDTH: u32 = 64;

/// Room left around the drawing inside the viewBox, in user units.
///
/// Labels are centred under their tower and this library cannot measure
/// text - there is no font metric on the host, and asking the browser for
/// one would put a measurement in the middle of a pure function. So the
/// margin is generous enough for the longest label a building name is
/// likely to be, and `text-anchor: middle` makes any overflow symmetric
/// rather than one-sided.
pub const MARGIN: i32 = 96;

/// How wide the city's grid is. Placement is a hash into this square, so
/// the extent decides how much room the hash has before two buildings
/// land on the same tile.
pub const CITY_EXTENT: u32 = 12;

/// Camera zoom stops. Three, because a continuous zoom invites a person to
/// hunt for the right level instead of reading the city.
pub const ZOOM_STOPS: [u32; 3] = [1, 2, 4];

/// Turns a tile coordinate into a point. Nothing else.
///
/// It used to carry a viewport, an extent and a pan offset, because the
/// drawing had to be fitted into a fixed bitmap and hit testing had to be
/// inverted out of the same numbers. The viewBox is fitted to the drawing
/// now and the browser does the hit testing, so a camera that still held a
/// viewport would be holding it for nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    /// Tile width in user units.
    pub tile_width: u32,
    /// Tile height: always half the width, by the 2:1 projection.
    pub tile_height: u32,
}

impl Camera {
    /// The one camera. There is nothing to fit, so there is nothing to
    /// choose.
    #[must_use]
    pub const fn tiles() -> Self {
        Self {
            tile_width: TILE_WIDTH,
            tile_height: TILE_WIDTH / TILE_RATIO,
        }
    }

    /// Projects a tile coordinate to a point.
    #[must_use]
    pub fn project(&self, u: i32, v: i32) -> (i32, i32) {
        let half_width = i32::try_from(self.tile_width.checked_div(2).unwrap_or(1)).unwrap_or(1);
        let half_height = i32::try_from(self.tile_height.checked_div(2).unwrap_or(1)).unwrap_or(1);
        (
            u.saturating_sub(v).saturating_mul(half_width),
            u.saturating_add(v).saturating_mul(half_height),
        )
    }
}

/// The window onto the drawing: `x y width height`, as a viewBox reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Frame {
    /// The attribute a viewBox takes.
    #[must_use]
    pub fn attr(&self) -> String {
        format!("{} {} {} {}", self.x, self.y, self.width, self.height)
    }
}

/// The window that shows the whole drawing at `stop` 0, and a portion of
/// it at each stop after that.
///
/// **Zoom has to crop rather than magnify.** With the window fitted to the
/// drawing, making every tile larger makes the window larger by the same
/// factor and the picture on screen does not move at all. So the tile is a
/// constant and a zoom stop divides the window instead, around the centre
/// the person has panned to.
///
/// A stop past the last one takes the last: a control that cannot go
/// further should stop rather than wrap round to the widest view.
#[must_use]
pub fn view_box(list: &DisplayList, stop: usize, pan: (i32, i32)) -> Frame {
    let points = list
        .ground
        .iter()
        .chain(list.faces.iter())
        .flat_map(|face| face.points.iter().copied())
        .chain(list.labels.iter().map(|label| label.at));
    let (mut low_x, mut low_y, mut high_x, mut high_y) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let mut any = false;
    for (x, y) in points {
        any = true;
        low_x = low_x.min(x);
        low_y = low_y.min(y);
        high_x = high_x.max(x);
        high_y = high_y.max(y);
    }
    if !any {
        // An empty city still needs a window, or the browser scales
        // nothing to fill everything.
        return Frame {
            x: -MARGIN,
            y: -MARGIN,
            width: u32::try_from(MARGIN.saturating_mul(2)).unwrap_or(1),
            height: u32::try_from(MARGIN.saturating_mul(2)).unwrap_or(1),
        };
    }
    let whole_width = u32::try_from(
        high_x
            .saturating_sub(low_x)
            .saturating_add(MARGIN.saturating_mul(2)),
    )
    .unwrap_or(1)
    .max(1);
    let whole_height = u32::try_from(
        high_y
            .saturating_sub(low_y)
            .saturating_add(MARGIN.saturating_mul(2)),
    )
    .unwrap_or(1)
    .max(1);
    let factor = ZOOM_STOPS
        .get(stop)
        .copied()
        .unwrap_or_else(|| ZOOM_STOPS.last().copied().unwrap_or(1))
        .max(1);
    let width = whole_width
        .checked_div(factor)
        .unwrap_or(whole_width)
        .max(1);
    let height = whole_height
        .checked_div(factor)
        .unwrap_or(whole_height)
        .max(1);
    // Centred on the middle of the drawing, then moved by what the person
    // panned. Panning at stop 0 does nothing visible, and that is correct:
    // the whole city is already in view.
    let centre_x = low_x.saturating_add(high_x).checked_div(2).unwrap_or(0);
    let centre_y = low_y.saturating_add(high_y).checked_div(2).unwrap_or(0);
    let half_width = i32::try_from(width.checked_div(2).unwrap_or(0)).unwrap_or(0);
    let half_height = i32::try_from(height.checked_div(2).unwrap_or(0)).unwrap_or(0);
    Frame {
        x: centre_x.saturating_sub(half_width).saturating_add(pan.0),
        y: centre_y.saturating_sub(half_height).saturating_add(pan.1),
        width,
        height,
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
    /// How many of those storeys the plan has finished.
    ///
    /// The reason the silhouette carries data at all: a tower's height is
    /// what its plan took on and this is the part that is done, so a person
    /// reads progress off the skyline instead of off a number beside it.
    /// A building with no denominator has no finished part either - the
    /// same refusal to invent a ratio that `Progress` makes in the type.
    pub done: u32,
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
pub fn prisms_of(
    buildings: &[BuildingProgress],
    busy: &BTreeSet<Address>,
    said: crate::lang::Lang,
) -> Vec<Prism> {
    let mut prisms: Vec<Prism> = buildings
        .iter()
        .map(|building| {
            let (u, v) = place(building.addr.as_str(), CITY_EXTENT);
            let storeys = storeys(scale_of(building.progress));
            Prism {
                id: building.addr.as_str().to_owned(),
                u,
                v,
                storeys,
                done: done_storeys(building.progress, storeys),
                active: busy.contains(&building.addr),
                // The words for a plan's progress come from the one
                // module that writes them, so the
                // label under a tower and the bar on a building's page
                // cannot drift apart.
                note: crate::progress::bar(
                    &building.progress,
                    false,
                    crate::progress::Subject::Plan,
                    said,
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

/// How many storeys of a tower are finished.
///
/// The plan's own ratio, carried onto the height the tower actually has.
/// Rounded down, and never the whole tower unless the plan is whole: a
/// building one row short of done should not look done from across the
/// city, which is the only distance this picture is read from.
fn done_storeys(progress: Progress, storeys: u32) -> u32 {
    let Progress::Planned(planned) = progress else {
        return 0;
    };
    let (done, total) = planned.ratio();
    if total == 0 {
        return 0;
    }
    if done >= total {
        return storeys;
    }
    storeys
        .checked_mul(done)
        .and_then(|scaled| scaled.checked_div(total))
        .unwrap_or(0)
        .min(storeys.saturating_sub(1))
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
    let extent = occupied_extent(&prisms);
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
        faces.extend(done_band_of(camera, &prism));
        faces.extend(windows_of(camera, &prism));
        labels.extend(labels_of(camera, &prism));
    }
    DisplayList {
        camera: *camera,
        ground: ground_of(camera, extent),
        faces,
        outline,
        labels,
    }
}

/// The ground the city stands on: the diamond of the whole extent, one
/// step lighter than the page behind it. Drawn first, so a building at
/// the far corner still reads as standing *on* something.
#[must_use]
pub fn ground_of(camera: &Camera, extent: u32) -> Vec<Face> {
    let last = i32::try_from(extent.saturating_sub(1)).unwrap_or_default();
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

/// The lit band up a tower's two walls: the part of the plan that is done.
///
/// Drawn over the walls rather than instead of them, from the base up, so
/// the reading is the one a person already has for a filled bar - except
/// that here the bar is the building. A tower with nothing finished gets
/// no band at all, which is not the same as a band of zero height: one
/// says nothing is done, the other would be drawing a claim about a plan
/// that has no denominator.
#[must_use]
pub fn done_band_of(camera: &Camera, prism: &Prism) -> Vec<Face> {
    if prism.done == 0 {
        return Vec::new();
    }
    let unit = storey_lift(camera);
    let done = unit.saturating_mul(i32::try_from(prism.done).unwrap_or(0));
    let [_, left, right] = faces_of(camera, prism, false);
    let mut band = Vec::new();
    for wall in [left, right] {
        // A wall runs top-edge, top-edge, base, base. The finished part
        // rises `done` from the base.
        let (Some(top_a), Some(top_b), Some(base_b), Some(base_a)) = (
            wall.points.first().copied(),
            wall.points.get(1).copied(),
            wall.points.get(2).copied(),
            wall.points.get(3).copied(),
        ) else {
            continue;
        };
        let raise = |point: (i32, i32)| (point.0, point.1.saturating_sub(done));
        let (head_a, head_b) = (raise(base_a), raise(base_b));
        // Never above the roof, whatever rounding did.
        let clamp = |head: (i32, i32), roof: (i32, i32)| (head.0, head.1.max(roof.1));
        band.push(Face {
            id: prism.id.clone(),
            token: "G7",
            points: [clamp(head_a, top_a), clamp(head_b, top_b), base_b, base_a],
        });
    }
    band
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

/// The `points` attribute of one face.
///
/// The one place a shape becomes an attribute, so a polygon written by
/// the ground loop and a polygon written by a tower cannot be spelled
/// differently.
#[must_use]
pub fn points_attr(points: &[(i32, i32); 4]) -> String {
    let mut out = String::new();
    for (index, (x, y)) in points.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{x},{y}"));
    }
    out
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
    // would hand a run the whole building's write domain. The room used
    // to be `room1` for every dispatch this page sent, so two pieces of
    // work started from the same tower wrote over each other's files.
    // The city opens a room from the name instead, and the name comes
    // from the work rather than from a counter (city-SPEC.md 8-13).
    // What a Dispatch frame looks like is `app::dispatch_command`'s
    // answer and only its answer - this page decides the address.
    crate::app::dispatch_command(
        &format!("{}/{}", building.trim(), session_name(task)),
        task,
        goal,
        "plan",
        // This page asks for two lines and a building; how hard to think
        // is chosen where the whole form is, at the bottom of the window.
        None,
    )
}

/// The name this page gives a session it starts: the first few words of
/// the task, which is what the person just wrote and will recognise in a
/// list of folders an hour later.
fn session_name(task: &str) -> String {
    let head: Vec<&str> = task.split_whitespace().take(4).collect();
    let joined = head.join(" ");
    // A name is one segment; anything the segment rules refuse is left
    // to `SessionName::parse`, which refuses the whole command rather
    // than inventing a spelling nobody typed.
    joined.replace(['/', '\\', ':'], "-")
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
/// The link into one building's own pages, said.
fn read_what(lang: crate::lang::Lang, id: &str) -> String {
    fill(say(lang, Msg::CityReadWhat), &[("id", id)])
}

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
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
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
                    status: word(Msg::AskingWhatItHolds).to_owned(),
                    what: word(Msg::CityScope).to_owned(),
                }
            }
        };
    };
    let prisms = prisms_of(&city.buildings, &busy, lang());
    let listing: Vec<(String, String)> = prisms
        .iter()
        .map(|prism| (prism.id.clone(), prism.note.clone()))
        .collect();
    let (dx, dy) = *pan.read();
    let camera = Camera::tiles();
    let list = draw(&camera, prisms.clone(), selected.as_deref());
    let frame = view_box(&list, *stop.read(), (dx, dy));
    let problems = unreadable_rows(&city.buildings);
    // The selected building's name, held twice outside the markup: the
    // submit closure keeps one for the length of the page, and the
    // disabled check reads another on every render. Empty when nothing
    // is selected, which is the case where the panel is not drawn.
    let submitting = selected.clone().unwrap_or_default();
    let checking = submitting.clone();
    let raised = city.buildings.len();
    let busy_now = city.active;
    rsx! {
        section { class: "city-view",
            crate::panel::Panel {
                title: if raised == 0 { word(Msg::CityNoBuildings).to_owned() }
                    else {
                        crate::lang::fill(
                            word(Msg::CityStanding),
                            &[("raised", &raised.to_string()), ("busy", &busy_now.to_string())],
                        )
                    },
                scope: word(Msg::CityTowerNote).to_owned(),
                source: word(Msg::CitySource).to_owned(),
            // An empty city still draws its ground, because a reader who
            // sees where buildings will stand knows what the page is for.
            // It draws less of it: at the full height the picture is a
            // 520px void above the one form that can end it.
            svg {
                class: if raised == 0 { "stage bare" } else { "stage" },
                view_box: "{frame.attr()}",
                preserve_aspect_ratio: "xMidYMid meet",
                role: "group",
                "aria-label": "{word(Msg::CityStageLabel)}",
                // A click that lands on no building clears the selection.
                // The groups below stop their own clicks here, so this is
                // the ground and only the ground.
                onclick: move |_| on_select.call(None),
                for face in list.ground.clone() {
                    polygon {
                        key: "g{face.points[0].0}-{face.points[0].1}",
                        points: "{points_attr(&face.points)}",
                        style: "fill:var(--{face.token})",
                    }
                }
                for prism in painter_order(prisms.clone()) {
                    g {
                        key: "{prism.id}",
                        class: if selected.as_deref() == Some(prism.id.as_str()) { "prism here" } else { "prism" },
                        tabindex: "0",
                        role: "button",
                        "aria-pressed": if selected.as_deref() == Some(prism.id.as_str()) { "true" } else { "false" },
                        "aria-label": "{prism.id}, {prism.note}",
                        onclick: {
                            let name = prism.id.clone();
                            move |event: MouseEvent| {
                                event.stop_propagation();
                                on_select.call(Some(name.clone()));
                            }
                        },
                        onkeydown: {
                            let name = prism.id.clone();
                            move |event: KeyboardEvent| {
                                // Enter and Space, the two keys a role of
                                // button owes a keyboard.
                                let pressed = match event.key() {
                                    Key::Enter => true,
                                    Key::Character(ref typed) => typed == " ",
                                    _ => false,
                                };
                                if pressed {
                                    event.prevent_default();
                                    on_select.call(Some(name.clone()));
                                }
                            }
                        },
                        title { "{prism.id} - {prism.note}" }
                        for (index , face) in faces_of(&camera, &prism, selected.as_deref() == Some(prism.id.as_str())).into_iter().enumerate() {
                            polygon {
                                key: "f{index}",
                                class: "body",
                                points: "{points_attr(&face.points)}",
                                style: "fill:var(--{face.token})",
                            }
                        }
                        for (index , face) in done_band_of(&camera, &prism).into_iter().enumerate() {
                            polygon {
                                key: "d{index}",
                                class: "done",
                                points: "{points_attr(&face.points)}",
                                style: "fill:var(--{face.token})",
                            }
                        }
                        for (index , face) in windows_of(&camera, &prism).into_iter().enumerate() {
                            polygon {
                                key: "w{index}",
                                points: "{points_attr(&face.points)}",
                                style: "fill:var(--{face.token})",
                            }
                        }
                    }
                }
                // Every label after every tower, because a label belongs to
                // the picture rather than to the building it names: drawn
                // inside its own group, a far building's name was painted
                // over by the near building in front of it. The group keeps
                // the name in its aria-label, so nothing is lost to a
                // reader who is not looking at pixels.
                for (index , label) in list.labels.clone().into_iter().enumerate() {
                    text {
                        key: "t{index}-{label.id}",
                        x: "{label.at.0}",
                        y: "{label.at.1}",
                        class: if label.leading { "name" } else { "note" },
                        style: "fill:var(--{label.token})",
                        "{label.text}"
                    }
                }
                if let Some(points) = list.outline {
                    // The one stroke in the picture, and the one place the
                    // accent appears here. On a canvas this had to settle
                    // for a grey, because `fillStyle` takes a value and a
                    // custom property is not one.
                    polygon {
                        class: "chosen",
                        points: "{points_attr(&points)}",
                    }
                }
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
                    placeholder: "{word(Msg::CityBuildingNamePlaceholder)}",
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
                    "{word(Msg::CityRaiseBuilding)}"
                }
            }
            if city.buildings.is_empty() {
                crate::panel::Empty {
                    status: word(Msg::CityNoBuildings).to_owned(),
                    what: word(Msg::CityNoBuildingsWhat).to_owned(),
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
                            "{word(Msg::ReadIt)}"
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
                    "{word(Msg::CityMoveLeft)}"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx.saturating_sub(PAN_STEP), dy)),
                    "{word(Msg::CityMoveRight)}"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx, dy.saturating_add(PAN_STEP))),
                    "{word(Msg::CityMoveUp)}"
                }
                button {
                    r#type: "button",
                    onclick: move |_| pan.set((dx, dy.saturating_sub(PAN_STEP))),
                    "{word(Msg::CityMoveDown)}"
                }
                button {
                    r#type: "button",
                    disabled: *stop.read() == 0 && (dx, dy) == (0, 0),
                    onclick: move |_| {
                        stop.set(0);
                        pan.set((0, 0));
                    },
                    "{word(Msg::CityFit)}"
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
                        "{read_what(lang(), &id)}"
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
                            placeholder: "{word(Msg::CityWhatShouldHappen)}",
                            value: "{task}",
                            oninput: move |event| task.set(event.value()),
                        }
                        input {
                            name: "goal",
                            placeholder: "{word(Msg::CityWhatCountsAsDone)}",
                            value: "{goal}",
                            oninput: move |event| goal.set(event.value()),
                        }
                        button {
                            r#type: "submit",
                            disabled: dispatch_command(&checking, &task.read(), &goal.read()).is_none(),
                            "{word(Msg::CitySendWorkHere)}"
                        }
                    }
                    button {
                        r#type: "button",
                        onclick: move |_| on_select.call(None),
                        "{word(Msg::CityClearSelection)}"
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
        let prisms = prisms_of(&buildings, &busy, crate::lang::Lang::En);
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
            prisms_of(&buildings, &busy, crate::lang::Lang::En),
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
        let prisms = prisms_of(&buildings, &BTreeSet::new(), crate::lang::Lang::En);
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
    fn sending_work_needs_a_room_and_a_task_and_nothing_else() {
        assert!(dispatch_command("lab", "fix the timer", "the test passes").is_some());
        assert!(
            dispatch_command("lab", "  ", "the test passes").is_none(),
            "a run with nothing to do is not a command"
        );
        assert!(
            dispatch_command("lab", "fix the timer", "").is_some(),
            "an empty goal is how this city spells a conversation, not a missing field"
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
        let channels::WireCommand::Dispatch { addr, session, .. } = *command else {
            panic!("the send-work form makes a dispatch");
        };
        assert_eq!(addr.as_str(), "lab");
        // The room is opened by the city from this name, and the name is
        // the work rather than a counter. Every dispatch from this page
        // used to go to `room1`, so the second piece of work started
        // from a tower wrote over the first one's files.
        let named = session.expect(
            "without a name the city has nothing to open a room from, and the run would hold the \
             whole building's write domain",
        );
        assert_eq!(named.as_str(), "fix the timer");

        let Some(ClientFrame::Command(second)) = dispatch_command(
            "lab",
            "fix the timer again, and this time read the failing case first",
            "the test passes",
        ) else {
            panic!("a complete form is a command");
        };
        let channels::WireCommand::Dispatch { session, .. } = *second else {
            panic!("the send-work form makes a dispatch");
        };
        assert_eq!(
            session.map(|name| name.as_str().to_owned()),
            Some("fix the timer again,".to_owned()),
            "a long task still yields a name short enough to be a folder"
        );
    }

    #[test]
    fn the_window_holds_everything_that_was_drawn() {
        // What replaced "a fitted city is inside the viewport it was
        // fitted to". The window is now derived from the drawing rather
        // than the drawing fitted into a window, so the property is
        // stronger: it cannot fail for a city of any shape.
        let list = draw(&Camera::tiles(), city(), None);
        let frame = view_box(&list, 0, (0, 0));
        let right = frame
            .x
            .saturating_add(i32::try_from(frame.width).unwrap_or(i32::MAX));
        let bottom = frame
            .y
            .saturating_add(i32::try_from(frame.height).unwrap_or(i32::MAX));
        for face in list.ground.iter().chain(list.faces.iter()) {
            for (x, y) in face.points {
                assert!(
                    x >= frame.x && x <= right && y >= frame.y && y <= bottom,
                    "({x},{y}) escaped the window {}",
                    frame.attr()
                );
            }
        }
    }

    #[test]
    fn zooming_crops_rather_than_magnifying_and_the_last_stop_stops() {
        // With the window fitted to the drawing, scaling every tile scales
        // the window with it and the picture on screen does not move. So a
        // stop has to take a smaller window, and this is the assertion
        // that keeps somebody from "fixing" it back into a tile scale.
        let list = draw(&Camera::tiles(), city(), None);
        let whole = view_box(&list, 0, (0, 0));
        let closer = view_box(&list, 1, (0, 0));
        let closest = view_box(&list, 2, (0, 0));
        assert!(closer.width < whole.width && closer.height < whole.height);
        assert!(closest.width < closer.width);
        // Past the end it stops rather than wrapping round to the widest
        // view: a control that cannot go further should stop.
        assert_eq!(view_box(&list, 99, (0, 0)), closest);
    }

    #[test]
    fn panning_moves_the_window_and_leaves_the_shapes_alone() {
        let list = draw(&Camera::tiles(), city(), None);
        let still = view_box(&list, 1, (0, 0));
        let moved = view_box(&list, 1, (PAN_STEP, -PAN_STEP));
        assert_eq!(moved.x, still.x + PAN_STEP);
        assert_eq!(moved.y, still.y - PAN_STEP);
        assert_eq!((moved.width, moved.height), (still.width, still.height));
        // The drawing itself does not know it was panned.
        assert_eq!(draw(&Camera::tiles(), city(), None), list);
    }

    #[test]
    fn a_tile_is_twice_as_wide_as_it_is_tall() {
        // Two halvings happen between a tile and a point, so an odd width
        // breaks the projection. The constant is the only place this can
        // now go wrong.
        let camera = Camera::tiles();
        assert_eq!(camera.tile_height * 2, camera.tile_width);
        assert_eq!(TILE_WIDTH % 4, 0);
    }

    #[test]
    fn the_lit_band_rises_with_the_plan_and_never_above_the_roof() {
        let camera = Camera::tiles();
        let tower = |done: u32| Prism {
            id: "lab".to_owned(),
            u: 0,
            v: 0,
            storeys: 4,
            done,
            active: false,
            note: String::new(),
        };
        let top_of = |prism: &Prism| {
            done_band_of(&camera, prism)
                .first()
                .map(|face| face.points[0].1)
        };
        let low = top_of(&tower(1)).unwrap();
        let high = top_of(&tower(3)).unwrap();
        assert!(high < low, "more done means the band reaches higher");
        let roof = faces_of(&camera, &tower(4), false)[0].points[0].1;
        assert!(
            top_of(&tower(4)).unwrap() >= roof,
            "a full band stops at the roof rather than growing past it"
        );
    }

    #[test]
    fn a_building_with_nothing_done_gets_no_band_rather_than_an_empty_one() {
        // Not the same statement: a band of zero height claims a ratio,
        // and a building with no denominator has none to claim.
        let camera = Camera::tiles();
        let unplanned = Prism {
            id: "lab".to_owned(),
            u: 0,
            v: 0,
            storeys: 1,
            done: 0,
            active: false,
            note: String::new(),
        };
        assert!(done_band_of(&camera, &unplanned).is_empty());
        assert_eq!(
            done_storeys(
                Progress::Unplanned(UnplannedProgress {
                    steps: 9,
                    budget: channels::BudgetUse::default(),
                }),
                4
            ),
            0
        );
    }

    #[test]
    fn a_plan_one_row_short_does_not_look_finished_from_across_the_city() {
        // The only distance this picture is read from.
        let planned = |done: u32, total: u32| planned("lab", done, total, Vec::new()).progress;
        assert_eq!(done_storeys(planned(6, 7), 4), 3, "short of done is short");
        assert_eq!(done_storeys(planned(7, 7), 4), 4, "and done is done");
    }

    #[test]
    fn one_shape_is_spelled_one_way() {
        assert_eq!(
            points_attr(&[(0, 1), (2, 3), (4, 5), (6, 7)]),
            "0,1 2,3 4,5 6,7"
        );
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
            done: 0,
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
            done: 0,
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
                    done: 0,
                    active: n == 0,
                    note: "3/7".to_owned(),
                }
            })
            .collect()
    }

    #[test]
    fn the_same_city_draws_the_same_shapes_in_the_same_order() {
        let camera = Camera::tiles();
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
    fn a_taller_building_stands_higher_and_a_selected_one_is_lighter() {
        let camera = Camera::tiles();
        let short = Prism {
            id: "a".to_owned(),
            u: 1,
            v: 1,
            storeys: 1,
            done: 0,
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
