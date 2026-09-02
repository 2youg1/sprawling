// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a city of buildings looks like: one prism each, as tall as the
//! work it holds, lit as far as the plan is done.
//!
//! Placement is a pure function of the building's id, so a city redraws
//! identically for every reader and a building does not move when its
//! neighbour is renamed. Height is logarithmic in the asset count, so
//! one giant does not flatten the rest of the city.
//!
//! Painter order is total: prisms are sorted before they are drawn, and
//! two renders of the same city produce byte-identical display lists.
//! That is the property that lets a headless test judge the picture.
//!
//! Where the shapes land is `web::isometry`'s; what the shapes mean is
//! here.

use std::collections::BTreeSet;

use channels::{Address, BuildingProgress, Progress};

use crate::isometry::{CITY_EXTENT, Camera, Edge, Face, Label, Part, ground_of, storey_lift};

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
            let edge = Edge {
                from: *from,
                to: *to,
            };
            for slot in 0i32..2 {
                let eighth = |offset: i32| Part {
                    num: slot.saturating_mul(4).saturating_add(offset),
                    den: 8,
                };
                let (near, far) = (eighth(1), eighth(3));
                let lit =
                    prism.active && storey.checked_rem(2) == Some(0) && wall == 0 && slot == 1;
                windows.push(Face {
                    id: prism.id.clone(),
                    token: if lit { "G9" } else { "G3" },
                    points: [
                        edge.at(near, head),
                        edge.at(far, head),
                        edge.at(far, foot),
                        edge.at(near, foot),
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
    use crate::isometry::Camera;
    use channels::{PlannedProgress, UnplannedProgress};

    fn planned(addr: &str, done: u32, total: u32, problems: Vec<String>) -> BuildingProgress {
        BuildingProgress {
            blocked: Vec::new(),
            ready: 0,
            addr: Address::parse(addr).unwrap(),
            progress: Progress::Planned(PlannedProgress {
                done,
                blocked: 0,
                total,
                done_ppb: 0,
                blocked_ppb: 0,
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
            blocked: Vec::new(),
            ready: 0,
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
