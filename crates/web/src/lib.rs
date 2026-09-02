// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

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
mod board;
mod building_view;
mod city_view;
mod command;
mod dashboard;
mod drop;
mod isometry;
mod keys;
mod lang;
mod ledger_view;
mod live;
mod pace;
mod palette;
mod panel;
mod phase;
mod progress;
mod prompt;
mod pursuit;
mod reach;
mod record;
mod route;
mod session;
mod sessions;
mod settings;
mod skyline;
mod socket;
mod theme;
mod turn;
mod vitals;
mod waiting;

pub use alert::{Alert, AlertKind, Alerts, Raise, Refused, absorb, alert_for, cleared_by, refused};
pub use app::watchable;
pub use app::{App, Backfill, DEFAULT_EFFORT, Destination, HELD_RECORDS, MODES, Root, Usage};
pub use app::{Lens, ProviderHealth, RunRow, Snapshot, View};
pub use app::{destinations, dispatch_command, hold, invalidated_by, showing};
pub use app::{latest_run, opened_building, rebuild};
pub use app::{render_tokens, render_usd, spend_line, status_line, waiting_line};
pub use app::{room_asked_for, started_here};
pub use approval::{ApprovalsView, BinRow, Cluster, RecycleBinView, ReturnPath};
pub use approval::{bin_rows, inbox, policy_admits, recycle_bin};
pub use archive_search::{ArchiveView, FILED_LATELY_MAX, Shelf};
pub use archive_search::{filed_at, filed_lately, filed_line, searchable, shelves};
pub use building_view::{BuildingView, Leaf, RoomQueue, day_label, opening_leaf};
pub use building_view::{room_addr, waiting_in};
pub use dashboard::CostsView;
pub use dashboard::{CostDimension, CostRow, SavingsRow, Trend, cost_rows, drawable, fold_line};
pub use dashboard::{SERIES_DASHES, SERIES_PER_CHART_MAX, SERIES_WIDTHS, share_per_mille};
pub use drop::{Dropped, Meaning, NAMED_FILES, Target, from_event, read, refusal};
pub use isometry::{Camera, Face, Frame, MARGIN, TILE_WIDTH, ZOOM_STOPS, points_attr, view_box};
pub use keys::{Act, Chord, Place, Stroke, press};
pub use lang::{Lang, Msg, Phrase, fill, phrase, say};
pub use ledger_view::{Filter, LedgerView, PAGE_ROWS, Page, Row};
pub use ledger_view::{export, kind_name, kind_named, page};
pub use live::cancel_command;
pub use live::{Feed, Line, LiveView, WINDOW, describe, describe_in, fork_command, short_run};
pub use pace::{Arrived, Paint, fold};
pub use palette::{Kind, Offer, Palette, matching};
pub use phase::{Phase, READING_ORDER};
pub use progress::distinguishable_without_colour;
pub use progress::{Bar, BarState, ProgressBar, Subject, bar};
pub use progress::{per_mille_of, track_token};
pub use prompt::{Block, Given, HASH_GLIMPSE, PromptView, Sighting, Skill, given, glimpse};
pub use reach::{ReachForm, configure_command, read_mounts, read_servers};
pub use reach::{show_mounts, show_servers};
pub use record::RecordView;
pub use route::{from_fragment, to_fragment};
pub use session::{Fact, SessionView, Tab, building_of, head_facts, room_for_link};
pub use sessions::{DEFAULT_MODE, ENDED_ROWS, Field, Plan, SeatRow, SessionsView};
pub use sessions::{counts_said, latest_room, listing, spent_of};
pub use settings::{AttachForm, AttachReadiness, EndpointRow, TagRow};
pub use settings::{can_dispatch, endpoint_rows, ready, tag_rows, url_is_safe};
pub use skyline::{DisplayList, Prism, done_band_of, draw, face_tokens, faces_of};
pub use skyline::{painter_order, place, storeys};
pub use socket::{Enrolment, enrol};
pub use socket::{Link, LinkAction, LinkEvent, LinkState, backoff_ms, read_frame, token_in};
#[cfg(target_arch = "wasm32")]
pub use socket::{open, pairing_token, send, socket_url};
pub use theme::{ACCENT_CHROMA_PERCENT, ALERT_CHROMA_PERCENT, CHROMA_COEFFICIENT};
pub use theme::{COLOUR_TOKENS, GRAY_CHROMA, GRAY_RAMP, HUE_ALERT, HUE_AXIS};
pub use theme::{CORNER_SCALES, continuity_order, superellipse_tenths};
pub use theme::{INFORMATION_FLOOR, L_CEILING, L_FLOOR, MOTION_QUICK_MS, PROGRESS_DONE};
pub use theme::{TEXT_SURFACE_CEILING, TEXT_TOKENS, TYPE_SCALE};
pub use theme::{custom_properties, gamut_chroma_ceiling, per_mille, resolved_chroma};
pub use turn::{Call, Note, OUTPUT_LINES, Outcome, Output, Turn, Used, opened_at, turns};
pub use vitals::{Sign, Vitals, signs};
pub use waiting::{Stalled, WaitingView, stalled};
