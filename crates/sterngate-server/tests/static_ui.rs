//! Regression guards for the embedded dashboard's markup and navigation.
//!
//! Two defects motivated these tests:
//!   * `switchTab()` validated against a hardcoded allow-list that was not
//!     updated when "Map Studio & Tuning" was added, so that button silently
//!     fell back to Live Telemetry.
//!   * `data-i18n` was placed on a element wrapping the safety modal's keyword
//!     `<code>` node; translating it assigned `textContent` to the parent and
//!     deleted that node, so every keyword-gated destructive action (AdBlue,
//!     SBC, and both flash paths) threw before the dialog could open.

use std::path::PathBuf;
use std::process::Command;

/// HTML elements that never have a closing tag, so they cannot wrap markup.
const VOID_ELEMENTS: &[&str] = &[
    "input", "img", "br", "hr", "meta", "link", "source", "area", "col",
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn static_dir() -> PathBuf {
    manifest_dir().join("static")
}

fn index_html() -> String {
    std::fs::read_to_string(static_dir().join("index.html")).expect("index.html must be readable")
}

/// Collect the `data-tab="..."` values declared by the nav strip.
fn declared_tabs(html: &str) -> Vec<String> {
    html.split("data-tab=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_string)
        .collect()
}

#[test]
fn every_nav_tab_has_a_matching_pane() {
    let html = index_html();
    let tabs = declared_tabs(&html);
    assert!(
        !tabs.is_empty(),
        "no data-tab attributes found in index.html"
    );

    for tab in &tabs {
        let pane_id = format!("id=\"tab-pane-{tab}\"");
        assert!(
            html.contains(&pane_id),
            "nav tab '{tab}' has no matching pane ({pane_id}) in index.html"
        );
    }
}

/// Drive the real `switchTab()` against a stubbed DOM and assert every nav
/// button activates its own pane. Skipped when `node` is unavailable.
#[test]
fn every_tab_button_opens_its_pane() {
    let harness = manifest_dir().join("tests/tabs_harness.js");

    let output = match Command::new("node")
        .arg(&harness)
        .arg(static_dir())
        .output()
    {
        Ok(output) => output,
        Err(_) => {
            eprintln!("skipping: `node` is not available on PATH");
            return;
        }
    };

    assert!(
        output.status.success(),
        "tab navigation regression detected:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// `applyTranslations()` assigns `textContent`, which replaces every child of
/// the target. An element carrying `data-i18n` must therefore be a leaf.
#[test]
fn no_translated_element_wraps_markup() {
    let html = index_html();
    let mut offenders = Vec::new();

    for (offset, _) in html.match_indices("data-i18n=\"") {
        // Walk back to the start of the element carrying the attribute.
        let Some(tag_start) = html[..offset].rfind('<') else {
            continue;
        };
        let tag_name: String = html[tag_start + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if VOID_ELEMENTS.contains(&tag_name.to_ascii_lowercase().as_str()) {
            continue;
        }

        // Anything other than a closing tag directly inside it is a child.
        let Some(open_end) = html[offset..].find('>').map(|i| offset + i + 1) else {
            continue;
        };
        let Some(next_tag) = html[open_end..].find('<').map(|i| open_end + i) else {
            continue;
        };
        if !html[next_tag..].starts_with("</") {
            let line = html[..offset].matches('\n').count() + 1;
            let key = html[offset + "data-i18n=\"".len()..]
                .split('"')
                .next()
                .unwrap_or("?");
            offenders.push(format!(
                "line {line}: <{tag_name} data-i18n=\"{key}\"> wraps child markup"
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "translating these elements would delete their children:\n  {}",
        offenders.join("\n  ")
    );
}
