// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Space and identity: buildings, residents, spine files, archive,
//! library, building policy, schedule, wizard.

mod archive;
mod building;
mod config_layers;
mod library;
mod policy;
mod resident;
mod room;
mod rules_tool;
mod schedule;
mod spine_files;
mod watch;
mod wizard;

pub use archive::ARCHIVE_DIR;
pub use archive::Entry as ArchiveEntry;
pub use archive::Kind as ArchiveKind;
pub use archive::day_of;
pub use archive::file as file_archive;
pub use archive::index as archive_index;
pub use building::adopt as adopt_building;
pub use building::adopted_payload as building_adopted_payload;
pub use building::created_payload as building_created_payload;
pub use building::{Building, BuildingTemplate, create as create_building};
pub use config_layers::path as config_path;
pub use config_layers::{CONFIG_FILE, ConfigLayer, Layer, load as load_config};
pub use config_layers::{write_effort, write_mcp, write_sandbox};
pub use library::{BUILDING_SHELF, Holding, LIBRARY_DIR, Library};
pub use policy::write_rules;
pub use policy::{BUILDING_FILE, BuildingRules, ModelPool, building_path, evaluate, load};
pub use resident::{Dossier, Identity, Resident, URBANITE_FILE, urbanite_path};
pub use room::open as open_room;
pub use rules_tool::RulesTool;
pub use schedule::{Cadence, Entry, SCHEDULE_FILE, Schedule, schedule_path};
pub use spine_files::{CITY_FILE, JOB_FILE, JobBrief, ROADMAP_FILE, RunBrief, handoff};
pub use spine_files::{job_path, norms, write_brief, write_job};
pub use watch::{Link, Source, WATCH_FILE, Watch, watch_path};
pub use wizard::{CityPlan, Relocation, Standing, relocate, survey};
