// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Where a point on the city's ground lands on the screen.
//!
//! One geometry, used by everything that draws and by everything that
//! picks: the browser hit-tests the very polygons this produced, so
//! "what is drawn is what can be picked" is the construction rather than
//! an assertion.
//!
//! Integer arithmetic throughout, and every operation saturating. The
//! picture must be identical on every machine that draws it, which is
//! what makes a display-list comparison a real test of the view.
//!
//! What this module does not know is what a building is. It projects
//! points, fits a window around what was drawn, and spells a polygon;
//! `web::skyline` is what turns a city into the prisms it projects.

use crate::skyline::DisplayList;

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

/// How much a storey lifts a prism: half a tile height, so a building of
/// three storeys is visibly taller than one of two at every zoom stop
/// without the tower leaving the viewport the camera fitted.
pub(crate) fn storey_lift(camera: &Camera) -> i32 {
    i32::try_from(camera.tile_height.checked_div(2).unwrap_or(1))
        .unwrap_or(1)
        .max(1)
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

/// The two points one edge of a shape runs between.
///
/// Named because everything that places something *on* a wall needs the
/// same pair, and passing two loose points is how a caller ends up
/// drawing a pane between one wall's near edge and another's far one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Edge {
    pub(crate) from: (i32, i32),
    pub(crate) to: (i32, i32),
}

/// How far along an edge a point sits: `num` parts out of `den`.
///
/// Kept as a pair rather than a ratio, because a ratio here would be a
/// float and this picture is integer throughout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Part {
    pub(crate) num: i32,
    pub(crate) den: i32,
}

impl Edge {
    /// The point `part` of the way along this edge, dropped by `fall`
    /// pixels. Integer throughout: the picture must be the same on every
    /// machine that draws it.
    pub(crate) fn at(self, part: Part, fall: i32) -> (i32, i32) {
        let step = |from: i32, to: i32| {
            to.saturating_sub(from)
                .saturating_mul(part.num)
                .checked_div(part.den.max(1))
                .unwrap_or_default()
                .saturating_add(from)
        };
        (
            step(self.from.0, self.to.0),
            step(self.from.1, self.to.1).saturating_add(fall),
        )
    }
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

/// How far one press of a pan control moves the city, in pixels. A whole
/// step rather than a smooth glide: the view is being read, not flown
/// through, and an animation here would ask for attention the page has
/// no reason to take.
pub const PAN_STEP: i32 = 64;
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
    use crate::skyline::{Prism, draw, place, storeys};

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
    fn one_shape_is_spelled_one_way() {
        assert_eq!(
            points_attr(&[(0, 1), (2, 3), (4, 5), (6, 7)]),
            "0,1 2,3 4,5 6,7"
        );
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
}
