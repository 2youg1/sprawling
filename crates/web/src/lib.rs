// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The only client, compiled to WebAssembly from Stage 4 on.
//! Depends on channels; nothing depends on it.
//!
//! Everything here except the transport shell compiles on the host, which
//! is what keeps `cargo clippy --workspace` and `cargo nextest` covering
//! this crate's logic even though its delivery target is `wasm32`.

mod alert;
mod app;
mod approval;
mod archive_search;
mod building_view;
mod city_view;
mod dashboard;
mod lang;
mod ledger_view;
mod live;
mod overview;
mod panel;
mod progress;
mod route;
mod settings;
mod socket;
mod theme;
mod vitals;

pub use alert::{Alert, AlertKind, Alerts, Raise, Refused, absorb, alert_for, cleared_by, refused};
pub use app::watchable;
pub use app::{App, Destination, HELD_RECORDS, NavGroup, Root, Usage};
pub use app::{ProviderHealth, RunPhase, RunRow, Snapshot, View};
pub use app::{destinations, dispatch_command, invalidated_by};
pub use app::{latest_run, opened_building, rebuild};
pub use app::{render_tokens, render_usd, spend_line, status_line, waiting_line};
pub use app::{room_asked_for, started_here};
pub use approval::{ApprovalsView, BinRow, Cluster, RecycleBinView, ReturnPath};
pub use approval::{bin_rows, inbox, policy_admits, recycle_bin, rollback_command};
pub use archive_search::{ArchiveView, FILED_LATELY_MAX, Shelf};
pub use archive_search::{filed_at, filed_lately, filed_line, searchable, shelves};
pub use building_view::{BuildingView, Leaf, RoomQueue, day_label, opening_leaf};
pub use building_view::{room_addr, waiting_in};
pub use city_view::{Camera, DisplayList, Face, Prism, ZOOM_STOPS};
pub use city_view::{Frame, MARGIN, TILE_WIDTH, done_band_of, points_attr, view_box};
pub use city_view::{draw, face_tokens, faces_of, painter_order, place, storeys};
pub use dashboard::CostsView;
pub use dashboard::{CostDimension, CostRow, SavingsRow, Trend, cost_rows, drawable, fold_line};
pub use dashboard::{SERIES_DASHES, SERIES_PER_CHART_MAX, SERIES_WIDTHS, share_per_mille};
pub use lang::{Lang, Msg, Phrase, fill, phrase, say};
pub use ledger_view::{Filter, LedgerView, PAGE_ROWS, Page, Row};
pub use ledger_view::{export, kind_name, kind_named, page};
pub use live::takeover_command;
pub use live::{Feed, Line, LiveView, WINDOW, describe, fork_command, short_run};
pub use overview::{Attention, OverviewView, Working, headline, needs_you, working};
pub use progress::distinguishable_without_colour;
pub use progress::{Bar, BarState, ProgressBar, Subject, bar};
pub use progress::{per_mille_of, track_token};
pub use route::{from_fragment, to_fragment};
pub use settings::{AttachForm, AttachReadiness, EndpointRow, TagRow};
pub use settings::{can_dispatch, endpoint_rows, ready, tag_rows, url_is_safe};
pub use socket::{Enrolment, enrol};
pub use socket::{Link, LinkAction, LinkEvent, LinkState, backoff_ms, read_frame};
#[cfg(target_arch = "wasm32")]
pub use socket::{open, send, socket_url};
pub use theme::{ACCENT_CHROMA_PERCENT, ALERT_CHROMA_PERCENT, CHROMA_COEFFICIENT};
pub use theme::{COLOUR_TOKENS, GRAY_CHROMA, GRAY_RAMP, HUE_ALERT, HUE_AXIS};
pub use theme::{CORNER_SCALES, continuity_order, superellipse_tenths};
pub use theme::{INFORMATION_FLOOR, L_CEILING, L_FLOOR, PROGRESS_DONE};
pub use theme::{custom_properties, gamut_chroma_ceiling, per_mille, resolved_chroma};
pub use vitals::{Sign, Vitals, signs};
