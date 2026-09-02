// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Render gate: a settled screen is opened in a real engine and measured
//! (v0.0.3 card V3.48).
//!
//! **This is the step the four-step method never had.** `ax` compares the
//! affordances both sides *wrote down*, and it says so itself: it is not a
//! computed tree, and looking at pixels is a person's job. That left one
//! whole class of defect with nothing watching it, because a stylesheet's
//! rules do not collide in either source file — they collide in the
//! cascade. Two rules that each read correctly where they are written laid
//! the composer out as a row and put a second left edge on every page, and
//! the tree was green through all of it: fourteen gates, 1,338 tests, and
//! a home page whose task box had floated into the top right corner.
//!
//! **What it asserts is a property, never a picture.** A screenshot
//! comparison fails on a font hint and passes on a page that is wrong in a
//! way nobody photographed. These three are the shape of a page rather
//! than its appearance, and each one is the generalisation of a defect
//! that shipped:
//!
//! 1. A page has one left edge. Every region in the centre column starts
//!    at the same x, so a heading and the panel under it cannot disagree.
//! 2. A panel's head is the top of its own panel, and starts at its left
//!    edge. A panel laid out as a row puts its title beside its body
//!    instead, which is exactly what a `form` did to the composer.
//! 3. Nothing is wider than the region that holds it.
//!
//! **A missing browser is a skip, not a red.** The engine is not in this
//! repository and cannot be: the gate says so on the line it prints and
//! judges nothing, because a gate that fails where it cannot look is a
//! gate somebody disables. `SPRAWLING_BROWSER` names one explicitly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::report::{Violation, XtaskError};
use crate::walk;

/// Where the settled screens live.
const SCREENS: &str = "crates/web/screens";

/// The design tokens `web::theme`'s own test writes out for the screens to
/// link. Without it a screen renders at browser defaults, which would make
/// every measurement below a measurement of nothing.
const TOKENS: &str = "target/screens/tokens.css";

/// Where the instrumented copies and the throwaway browser profile go.
const WORK: &str = "target/render";

/// The window the screens are judged in, in CSS pixels.
///
/// One size rather than a sweep: the properties asserted here are true at
/// every width, and a second viewport would double the run time to
/// re-check the same three facts. A narrow-window rule (what wraps, what
/// collapses) is a different gate and does not exist yet.
const VIEWPORT: (u32, u32) = (1440, 1200);

/// The element the probe writes its measurements into.
const SINK: &str = "sprawling-render";

/// Two boxes may differ by this many pixels and still count as aligned.
///
/// Sub-pixel layout rounds, and a border that a rule paints on one side
/// only moves a box by one. Anything larger is a decision somebody made
/// twice.
const SLACK: i64 = 1;

/// One measured box, in the page's own coordinates.
struct Box {
    kind: String,
    tag: String,
    class: String,
    left: i64,
    top: i64,
    width: i64,
    height: i64,
}

impl Box {
    /// The x this box ends at.
    fn right(&self) -> i64 {
        self.left.saturating_add(self.width)
    }

    /// What a violation calls this box.
    fn name(&self) -> String {
        format!("{}.{}", self.tag.to_lowercase(), self.class)
    }

    /// A box with no area is a container that holds nothing on this
    /// screen, and it has no position worth comparing.
    fn drawn(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let screens = root.join(SCREENS);
    if !screens.is_dir() {
        return Ok(Vec::new());
    }
    if !root.join(TOKENS).is_file() {
        println!("gate render: {TOKENS} is not written; run `cargo test -p web` first (skipped)");
        return Ok(Vec::new());
    }
    let Some(browser) = browser() else {
        println!("gate render: no headless browser found; set SPRAWLING_BROWSER to one (skipped)");
        return Ok(Vec::new());
    };
    let engine = Engine::new(root, browser)?;
    let mut violations = Vec::new();
    for path in walk::files_with_ext(&screens, &["html"])? {
        let rel = walk::rel(root, &path);
        let boxes = engine.measure(&path, &rel)?;
        judge(&rel, &boxes, &mut violations);
    }
    Ok(violations)
}

/// What it takes to render one screen: the tree the screens live in, the
/// engine that draws them, and the scratch directory the instrumented
/// copies go to. The three always travel together, so they have a name.
struct Engine<'tree> {
    root: &'tree Path,
    browser: PathBuf,
    work: PathBuf,
}

/// The engine to render with: named by the environment, or the first one
/// of the three desktops' own browsers that is installed.
fn browser() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("SPRAWLING_BROWSER") {
        let path = PathBuf::from(named);
        if path.is_file() {
            return Some(path);
        }
    }
    const FIXED: [&str; 6] = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/usr/bin/chromium",
    ];
    for candidate in FIXED {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    on_path(&[
        "google-chrome",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
    ])
}

/// The first of these names that is executable somewhere on `PATH`.
fn on_path(names: &[&str]) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

impl<'tree> Engine<'tree> {
    /// Prepare the scratch directory the instrumented copies go to.
    fn new(root: &'tree Path, browser: PathBuf) -> Result<Self, XtaskError> {
        let work = root.join(WORK);
        std::fs::create_dir_all(&work).map_err(|source| XtaskError::Io {
            path: walk::rel(root, &work),
            source,
        })?;
        Ok(Self {
            root,
            browser,
            work,
        })
    }

    /// Render one screen and read its boxes back.
    fn measure(&self, screen: &Path, rel: &str) -> Result<Vec<Box>, XtaskError> {
        let name = screen
            .file_name()
            .and_then(|held| held.to_str())
            .unwrap_or("screen.html");
        let body = walk::read_text(screen)?;
        let source = screen.parent().unwrap_or(self.root);
        let instrumented = self.work.join(name);
        std::fs::write(&instrumented, instrument(&body, source)).map_err(|source| {
            XtaskError::Io {
                path: walk::rel(self.root, &instrumented),
                source,
            }
        })?;
        let profile = self.work.join("profile");
        let output = Command::new(&self.browser)
            .arg("--headless=new")
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg("--hide-scrollbars")
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("--window-size={},{}", VIEWPORT.0, VIEWPORT.1))
            .arg("--virtual-time-budget=4000")
            .arg("--dump-dom")
            .arg(url_of(&instrumented))
            .output()
            .map_err(|err| XtaskError::Cmd {
                cmd: format!("{} --dump-dom", self.browser.display()),
                msg: err.to_string(),
            })?;
        let dom = String::from_utf8_lossy(&output.stdout);
        let Some(records) = sink(&dom) else {
            return Err(XtaskError::Cmd {
                cmd: format!("{} --dump-dom {rel}", self.browser.display()),
                msg: "the probe wrote nothing; the engine rendered no page or ran no script"
                    .to_owned(),
            });
        };
        let boxes: Vec<Box> = records.split(" ; ").filter_map(parse_box).collect();
        if boxes.is_empty() {
            return Err(XtaskError::Cmd {
                cmd: format!("{} --dump-dom {rel}", self.browser.display()),
                msg: "the page rendered no centre column and no panel".to_owned(),
            });
        }
        Ok(boxes)
    }
}

/// The measurements the probe left in the document, if it ran.
fn sink(dom: &str) -> Option<&str> {
    let open = format!("<pre id=\"{SINK}\">");
    let start = dom.find(&open)?.checked_add(open.len())?;
    let rest = dom.get(start..)?;
    let end = rest.find("</pre>")?;
    rest.get(..end)
}

/// `kind tag class left top width height`, as the probe writes it.
fn parse_box(record: &str) -> Option<Box> {
    let mut field = record.split_whitespace();
    let kind = field.next()?.to_owned();
    let tag = field.next()?.to_owned();
    let class = field.next()?.to_owned();
    let left = field.next()?.parse().ok()?;
    let top = field.next()?.parse().ok()?;
    let width = field.next()?.parse().ok()?;
    let height = field.next()?.parse().ok()?;
    Some(Box {
        kind,
        tag,
        class,
        left,
        top,
        width,
        height,
    })
}

/// Rewrite the stylesheet links to absolute file URLs and append the
/// probe.
///
/// The links are rewritten rather than the copy being written beside the
/// screen: a temporary file inside `crates/web/screens` is a file the
/// other gates walk, and one left behind by an interrupted run would be
/// judged as a screen.
fn instrument(body: &str, source: &Path) -> String {
    let mut out = String::with_capacity(body.len().saturating_add(PROBE.len()));
    let mut rest = body;
    while let Some(at) = rest.find("href=\"") {
        let Some(head) = rest.get(..at) else { break };
        let Some(tail) = rest.get(at.saturating_add(6)..) else {
            break;
        };
        let Some(close) = tail.find('"') else { break };
        let Some(value) = tail.get(..close) else {
            break;
        };
        out.push_str(head);
        out.push_str("href=\"");
        if value.ends_with(".css") {
            out.push_str(&url_of(&source.join(value)));
        } else {
            out.push_str(value);
        }
        out.push('"');
        rest = tail.get(close.saturating_add(1)..).unwrap_or("");
    }
    out.push_str(rest);
    match out.rfind("</body>") {
        Some(at) => {
            let (head, tail) = out.split_at(at);
            format!("{head}{PROBE}{tail}")
        }
        None => format!("{out}{PROBE}"),
    }
}

/// A `file://` URL for a path, in the form every engine accepts.
fn url_of(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = absolute.display().to_string();
    let cleaned = text
        .strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .replace('\\', "/");
    if cleaned.starts_with('/') {
        format!("file://{cleaned}")
    } else {
        format!("file:///{cleaned}")
    }
}

/// The three properties, in the order a reader meets them on the page.
fn judge(rel: &str, boxes: &[Box], out: &mut Vec<Violation>) {
    let centre = boxes.iter().find(|held| held.kind == "centre");
    one_left_edge(rel, boxes, out);
    heads_lead_their_panels(rel, boxes, out);
    if let Some(centre) = centre {
        nothing_overflows(rel, centre, boxes, out);
    }
}

/// Every region in the centre column starts at the same x.
fn one_left_edge(rel: &str, boxes: &[Box], out: &mut Vec<Violation>) {
    let mut edges: BTreeMap<i64, String> = BTreeMap::new();
    for region in boxes
        .iter()
        .filter(|held| held.kind == "region" && held.drawn())
    {
        edges.entry(region.left).or_insert_with(|| region.name());
    }
    if edges.len() < 2 {
        return;
    }
    let listed: Vec<String> = edges
        .iter()
        .map(|(left, name)| format!("{name} at x={left}"))
        .collect();
    if let (Some(first), Some(last)) = (edges.keys().next(), edges.keys().next_back())
        && last.saturating_sub(*first) <= SLACK
    {
        return;
    }
    out.push(Violation {
        gate: "render",
        location: rel.to_owned(),
        rule: "a page has one left edge: every region in the centre column starts at the same x"
            .to_owned(),
        violation: format!("this page has {}: {}", edges.len(), listed.join(", ")),
        alternative: "let one authority set the inline margins - a panel states its vertical \
                      rhythm, the region states the spine"
            .to_owned(),
    });
}

/// A panel's head is the topmost of its parts and starts at its left edge.
fn heads_lead_their_panels(rel: &str, boxes: &[Box], out: &mut Vec<Violation>) {
    let mut panel: Option<&Box> = None;
    let mut parts: Vec<&Box> = Vec::new();
    for held in boxes {
        match held.kind.as_str() {
            "panel" => {
                if let Some(open) = panel.take() {
                    head_leads(rel, open, &parts, out);
                }
                parts.clear();
                panel = Some(held);
            }
            "part" => parts.push(held),
            _ => {}
        }
    }
    if let Some(open) = panel {
        head_leads(rel, open, &parts, out);
    }
}

/// One panel's own judgement.
fn head_leads(rel: &str, panel: &Box, parts: &[&Box], out: &mut Vec<Violation>) {
    let drawn: Vec<&&Box> = parts.iter().filter(|held| held.drawn()).collect();
    let Some(head) = drawn
        .iter()
        .find(|held| held.class.split('.').any(|word| word == "panel-head"))
    else {
        return;
    };
    if let Some(above) = drawn
        .iter()
        .find(|held| head.top.saturating_sub(held.top) > SLACK)
    {
        out.push(Violation {
            gate: "render",
            location: rel.to_owned(),
            rule: "a panel's head is the top of its own panel".to_owned(),
            violation: format!(
                "{} sits at y={} while {} is at y={} inside {}",
                head.name(),
                head.top,
                above.name(),
                above.top,
                panel.name()
            ),
            alternative: "the panel grammar stacks: state the panel's own display so an \
                          element rule cannot lay its parts out in a row"
                .to_owned(),
        });
    }
    if head.left.saturating_sub(panel.left).abs() > SLACK {
        out.push(Violation {
            gate: "render",
            location: rel.to_owned(),
            rule: "a panel's head starts at its panel's left edge".to_owned(),
            violation: format!(
                "{} starts at x={} inside {} at x={}",
                head.name(),
                head.left,
                panel.name(),
                panel.left
            ),
            alternative: "give the head no inline offset of its own".to_owned(),
        });
    }
}

/// Nothing measured reaches past the region that holds it.
fn nothing_overflows(rel: &str, centre: &Box, boxes: &[Box], out: &mut Vec<Violation>) {
    for held in boxes
        .iter()
        .filter(|held| held.kind != "centre" && held.drawn())
    {
        if held.right() > centre.right().saturating_add(SLACK) {
            out.push(Violation {
                gate: "render",
                location: rel.to_owned(),
                rule: "nothing is wider than the region that holds it".to_owned(),
                violation: format!(
                    "{} ends at x={} and its region ends at x={}",
                    held.name(),
                    held.right(),
                    centre.right()
                ),
                alternative: "bound it with the page width token rather than the window".to_owned(),
            });
        }
    }
}

/// The measuring script.
///
/// It writes one line per box into a `<pre>` the dump then carries back,
/// because `--dump-dom` returns the document and nothing else: anything
/// the gate wants to know has to be in the document when it is dumped.
const PROBE: &str = r#"<pre id="sprawling-render"></pre>
<script>
(function(){
  var out = [];
  function box(kind, node) {
    var r = node.getBoundingClientRect();
    var cls = (node.getAttribute('class') || '-').trim().split(/\s+/).join('.');
    out.push([kind, node.tagName, cls,
              Math.round(r.left), Math.round(r.top),
              Math.round(r.width), Math.round(r.height)].join(' '));
  }
  var centre = document.querySelector('.centre');
  if (centre) {
    box('centre', centre);
    for (var i = 0; i < centre.children.length; i++) { box('region', centre.children[i]); }
  }
  var panels = document.querySelectorAll('.panel');
  for (var p = 0; p < panels.length; p++) {
    box('panel', panels[p]);
    var parts = panels[p].children;
    for (var j = 0; j < parts.length; j++) { box('part', parts[j]); }
  }
  document.getElementById('sprawling-render').textContent = out.join(' ; ');
})();
</script>
"#;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::{Box, judge, parse_box, sink};

    fn boxed(kind: &str, class: &str, left: i64, top: i64, width: i64) -> Box {
        Box {
            kind: kind.to_owned(),
            tag: "DIV".to_owned(),
            class: class.to_owned(),
            left,
            top,
            width,
            height: 40,
        }
    }

    #[test]
    fn a_second_left_edge_is_caught() {
        // The defect this pins: `.panel`'s shorthand margin reset the
        // inline auto margins the centre column had set, so a page's
        // heading was centred and the panels under it were not.
        let boxes = vec![
            boxed("centre", "centre", 200, 0, 1146),
            boxed("region", "record-head", 284, 60, 1040),
            boxed("region", "panel", 253, 160, 1040),
        ];
        let mut out = Vec::new();
        judge("record.html", &boxes, &mut out);
        assert_eq!(out.len(), 1, "one page, one edge");
        assert!(out.iter().any(|v| v.violation.contains("x=284")));
    }

    #[test]
    fn a_head_laid_out_beside_its_body_is_caught() {
        // The defect this pins: `form { display: flex }` captured the
        // composer, so its head sat beside the box instead of above it.
        let boxes = vec![
            boxed("centre", "centre", 200, 0, 1146),
            boxed("region", "panel.composer", 253, 60, 1040),
            boxed("panel", "panel.composer", 253, 60, 1040),
            boxed("part", "panel-body", 755, 60, 400),
            boxed("part", "panel-head", 253, 205, 400),
        ];
        let mut out = Vec::new();
        judge("sessions.html", &boxes, &mut out);
        assert!(
            out.iter()
                .any(|v| v.rule.contains("the top of its own panel")),
            "a head below its own body is the row layout"
        );
    }

    #[test]
    fn a_page_whose_regions_agree_is_clean() {
        let boxes = vec![
            boxed("centre", "centre", 200, 0, 1146),
            boxed("region", "record-head", 253, 60, 1040),
            boxed("region", "panel", 253, 160, 1040),
            boxed("panel", "panel", 253, 160, 1040),
            boxed("part", "panel-head", 253, 160, 400),
            boxed("part", "panel-body", 253, 200, 400),
        ];
        let mut out = Vec::new();
        judge("record.html", &boxes, &mut out);
        assert!(out.is_empty(), "{:?}", out.first().map(|v| &v.violation));
    }

    #[test]
    fn a_box_wider_than_its_region_is_caught() {
        let boxes = vec![
            boxed("centre", "centre", 200, 0, 1000),
            boxed("region", "panel", 200, 60, 2376),
        ];
        let mut out = Vec::new();
        judge("sessions.html", &boxes, &mut out);
        assert!(out.iter().any(|v| v.rule.contains("wider than the region")));
    }

    #[test]
    fn the_probe_writes_where_the_gate_reads() {
        let dom = "<html><body><pre id=\"sprawling-render\">region DIV a 1 2 3 4</pre></body>";
        let records = sink(dom).expect("the sink is found");
        let held = parse_box(records).expect("one record parses");
        assert_eq!((held.left, held.top, held.width, held.height), (1, 2, 3, 4));
    }
}
