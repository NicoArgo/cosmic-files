// SPDX-License-Identifier: GPL-3.0-only

//! Folder colors, taken from the terminal's per-folder rules.
//!
//! COSMIC Terminal lets a folder carry an identity — a name and a color — in
//! `com.system76.CosmicTerm/v1/dir_rules`. The file manager reads that same
//! key rather than keeping a list of its own, so a folder has one color and
//! one place to change it: pick it in the terminal and the folder's icon here
//! follows, live.
//!
//! Only the fields that decide *which folder* and *what color* are read. Serde
//! skips the rest, so the terminal can grow its rules without this side
//! noticing.

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use cosmic::{iced::Color, widget};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, RwLock},
};

pub const TERM_CONFIG_ID: &str = "com.system76.CosmicTerm";
pub const TERM_CONFIG_VERSION: u64 = 1;

/// The subset of the terminal's `DirRule` that says where a color goes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct TermDirRule {
    pub path: String,
    pub include_subdirs: bool,
    pub enabled: bool,
    pub accent: Option<String>,
}

impl Default for TermDirRule {
    fn default() -> Self {
        Self {
            path: String::new(),
            include_subdirs: false,
            enabled: true,
            accent: None,
        }
    }
}

#[derive(Clone, CosmicConfigEntry, Debug, Default, Eq, PartialEq)]
#[version = 1] // TERM_CONFIG_VERSION
pub struct TermRules {
    pub dir_rules: BTreeMap<u64, TermDirRule>,
}

impl TermRules {
    pub fn load() -> Self {
        match cosmic_config::Config::new(TERM_CONFIG_ID, TERM_CONFIG_VERSION) {
            Ok(handler) => Self::get_entry(&handler).unwrap_or_else(|(errs, rules)| {
                log::info!("errors loading terminal folder rules: {errs:?}");
                rules
            }),
            Err(err) => {
                log::warn!("failed to open terminal config: {err}");
                Self::default()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#RRGGBB`, with or without the `#`; a trailing alpha pair is ignored.
    pub fn parse(hex: &str) -> Option<Self> {
        let hex = hex.trim();
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Self(byte(0)?, byte(2)?, byte(4)?))
    }

    /// Mix toward black: `amount` 0.0 keeps the color, 1.0 is black.
    fn darken(self, amount: f32) -> Self {
        let f = |c: u8| (f32::from(c) * (1.0 - amount)).round() as u8;
        Self(f(self.0), f(self.1), f(self.2))
    }

    fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    pub fn color(self) -> Color {
        Color::from_rgb8(self.0, self.1, self.2)
    }
}

/// A rule reduced to what matching needs.
#[derive(Clone, Debug, PartialEq)]
struct ColorRule {
    path: PathBuf,
    include_subdirs: bool,
    color: Rgb,
}

fn absolute_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Same expansion as the terminal: `~` and `~/...` only.
    if let Some(rest) = trimmed.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with('/') {
            return Some(dirs::home_dir()?.join(rest.trim_start_matches('/')));
        }
        return None;
    }
    let path = PathBuf::from(trimmed);
    path.is_absolute().then_some(path)
}

fn color_rules(rules: &TermRules) -> Vec<ColorRule> {
    rules
        .dir_rules
        .values()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            Some(ColorRule {
                path: absolute_path(&rule.path)?,
                include_subdirs: rule.include_subdirs,
                color: Rgb::parse(rule.accent.as_deref()?)?,
            })
        })
        .collect()
}

/// How many components of `rule_path` matched `dir`, or `None` when the rule
/// does not reach it. Compared by component, so `/a` never matches `/ab`.
fn match_depth(rule_path: &Path, dir: &Path, include_subdirs: bool) -> Option<usize> {
    let mut depth = 0;
    let mut dir_components = dir.components();
    for rule_component in rule_path.components() {
        if dir_components.next()? != rule_component {
            return None;
        }
        depth += 1;
    }
    if dir_components.next().is_some() && !include_subdirs {
        return None;
    }
    Some(depth)
}

/// The color the terminal would give `dir`: its own rule, else the deepest
/// rule that opted into covering its tree. Ties go to the lowest id, which is
/// the order the rules were collected in.
fn resolve(rules: &[ColorRule], dir: &Path) -> Option<Rgb> {
    let mut best: Option<(usize, Rgb)> = None;
    for rule in rules {
        let Some(depth) = match_depth(&rule.path, dir, rule.include_subdirs) else {
            continue;
        };
        if best.is_none_or(|(best_depth, _)| depth > best_depth) {
            best = Some((depth, rule.color));
        }
    }
    best.map(|(_, color)| color)
}

// Global because folder icons are built off the UI thread, in the scanners,
// which only get a path. Loaded on first use so every entry point — the app,
// the file dialog, the desktop — sees the rules without being wired up.
static RULES: LazyLock<RwLock<Vec<ColorRule>>> =
    LazyLock::new(|| RwLock::new(color_rules(&TermRules::load())));

/// Recolored icons, by (theme icon file, color). Generated once each.
static ICONS: LazyLock<Mutex<FxHashMap<(PathBuf, Rgb), widget::icon::Handle>>> =
    LazyLock::new(|| Mutex::new(FxHashMap::default()));

/// Replace the rules. Returns whether any folder's color could have changed.
pub fn set_rules(rules: &TermRules) -> bool {
    let rules = color_rules(rules);
    let mut current = RULES.write().unwrap();
    if *current == rules {
        return false;
    }
    *current = rules;
    true
}

/// The color of `dir`, if a rule gives it one.
pub fn color_for(dir: &Path) -> Option<Rgb> {
    let rules = RULES.read().unwrap();
    if rules.is_empty() {
        return None;
    }
    resolve(&rules, dir)
}

/// The greys of COSMIC's folder icons: back tab (two stops), then front.
const BACK_BOTTOM: &str = "#484848";
const BACK_TOP: &str = "#636363";
const FRONT_LIGHT: &str = "#979FAD";
const FRONT_DARK: &str = "#808080";

fn replace_ci(svg: &str, from: &str, to: &str) -> (String, bool) {
    let upper = from.to_ascii_uppercase();
    let lower = from.to_ascii_lowercase();
    let found = svg.contains(&upper) || svg.contains(&lower);
    (svg.replace(&upper, to).replace(&lower, to), found)
}

/// The theme's folder with its greys swapped for shades of `color`: the front
/// is the color itself going slightly darker, the back tab is well darker, the
/// same depth the grey original has. `None` when the icon is not drawn with
/// those greys, so we never return something that only half changed.
pub fn recolor_svg(svg: &str, color: Rgb) -> Option<String> {
    let (svg, front) = replace_ci(svg, FRONT_LIGHT, &color.hex());
    let (svg, _) = replace_ci(&svg, FRONT_DARK, &color.darken(0.14).hex());
    let (svg, _) = replace_ci(&svg, BACK_TOP, &color.darken(0.32).hex());
    let (svg, _) = replace_ci(&svg, BACK_BOTTOM, &color.darken(0.45).hex());
    front.then_some(svg)
}

/// Stand-in for icon themes whose folders cannot be recolored (a raster
/// theme, or one drawn with other colors): a plain folder in the rule's
/// color. Losing the theme's drawing beats losing the folder's color.
fn fallback_svg(color: Rgb) -> String {
    let back = color.darken(0.4).hex();
    let front = color.hex();
    format!(
        r#"<svg width="256" height="256" viewBox="0 0 256 256" xmlns="http://www.w3.org/2000/svg"><path d="M8 58a16 16 0 0 1 16-16h72l24 16h112a16 16 0 0 1 16 16v124a16 16 0 0 1-16 16H24a16 16 0 0 1-16-16z" fill="{back}"/><rect x="8" y="74" width="240" height="140" rx="16" fill="{front}"/></svg>"#
    )
}

/// The folder icon `name` (a themed icon name, like `folder` or
/// `folder-documents`) painted in `color`.
pub fn colored_icon(name: &str, icon_size: u16, color: Rgb) -> widget::icon::Handle {
    let themed = widget::icon::from_name(name)
        .prefer_svg(true)
        .size(icon_size)
        .path();
    let key = (themed.clone().unwrap_or_default(), color);
    if let Some(handle) = ICONS.lock().unwrap().get(&key) {
        return handle.clone();
    }

    let svg = themed
        .filter(|path| path.extension().is_some_and(|ext| ext == "svg"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|svg| recolor_svg(&svg, color))
        .unwrap_or_else(|| fallback_svg(color));
    let handle = widget::icon::from_svg_bytes(svg.into_bytes());
    ICONS.lock().unwrap().insert(key, handle.clone());
    handle
}

/// Paint a symbolic folder icon (the sidebar's) in `dir`'s color, if it has one.
pub fn tint_symbolic(icon: widget::icon::Icon, dir: &Path) -> widget::icon::Icon {
    match color_for(dir) {
        Some(rgb) => {
            let color = rgb.color();
            icon.class(cosmic::theme::Svg::custom(move |_| {
                cosmic::iced::widget::svg::Style { color: Some(color) }
            }))
        }
        None => icon,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(path: &str, include_subdirs: bool, accent: &str) -> TermDirRule {
        TermDirRule {
            path: path.into(),
            include_subdirs,
            enabled: true,
            accent: Some(accent.into()),
        }
    }

    fn rules(list: Vec<TermDirRule>) -> Vec<ColorRule> {
        color_rules(&TermRules {
            dir_rules: list.into_iter().enumerate().map(|(i, r)| (i as u64, r)).collect(),
        })
    }

    #[test]
    fn parses_hex() {
        assert_eq!(Rgb::parse("#48B9C7"), Some(Rgb(0x48, 0xB9, 0xC7)));
        assert_eq!(Rgb::parse("48b9c7"), Some(Rgb(0x48, 0xB9, 0xC7)));
        assert_eq!(Rgb::parse("#48B9C7FF"), Some(Rgb(0x48, 0xB9, 0xC7)));
        assert_eq!(Rgb::parse("#48B9C"), None);
        assert_eq!(Rgb::parse("#GGGGGG"), None);
        assert_eq!(Rgb::parse("#ÁÁÁ"), None);
    }

    #[test]
    fn a_rule_covers_its_folder_only() {
        let rules = rules(vec![rule("/a", false, "#FF0000")]);
        assert_eq!(resolve(&rules, Path::new("/a")), Some(Rgb(255, 0, 0)));
        assert_eq!(resolve(&rules, Path::new("/a/")), Some(Rgb(255, 0, 0)));
        assert_eq!(resolve(&rules, Path::new("/a/b")), None);
        assert_eq!(resolve(&rules, Path::new("/ab")), None);
        assert_eq!(resolve(&rules, Path::new("/")), None);
    }

    #[test]
    fn subtree_rules_reach_down_and_the_deepest_wins() {
        let rules = rules(vec![
            rule("/a", true, "#FF0000"),
            rule("/a/b", false, "#00FF00"),
        ]);
        assert_eq!(resolve(&rules, Path::new("/a/x/y")), Some(Rgb(255, 0, 0)));
        assert_eq!(resolve(&rules, Path::new("/a/b")), Some(Rgb(0, 255, 0)));
        // `/a/b` does not cover its own tree, so `/a` does.
        assert_eq!(resolve(&rules, Path::new("/a/b/c")), Some(Rgb(255, 0, 0)));
    }

    #[test]
    fn disabled_colorless_and_relative_rules_are_skipped() {
        let mut off = rule("/a", false, "#FF0000");
        off.enabled = false;
        let mut no_color = rule("/b", false, "#FF0000");
        no_color.accent = None;
        let rules = rules(vec![off, no_color, rule("rel", false, "#FF0000")]);
        assert!(rules.is_empty());
    }

    #[test]
    fn reads_the_terminal_format() {
        let ron = r##"{
            1: (
                path: "/home/me/proj",
                include_subdirs: false,
                enabled: true,
                syntax_theme_dark: None,
                opacity: None,
                tab_title: Some("PROJ"),
                accent: Some("#48B9C7"),
            ),
        }"##;
        let parsed: BTreeMap<u64, TermDirRule> = ron::from_str(ron).unwrap();
        assert_eq!(parsed[&1].accent.as_deref(), Some("#48B9C7"));
        assert_eq!(parsed[&1].path, "/home/me/proj");
    }

    #[test]
    fn recolors_every_grey_and_refuses_foreign_icons() {
        let svg = r##"<stop stop-color="#484848"/><stop stop-color="#636363"/>
            <stop stop-color="#979FAD"/><stop stop-color="#808080"/>"##;
        let out = recolor_svg(svg, Rgb(0x48, 0xB9, 0xC7)).unwrap();
        for grey in [BACK_BOTTOM, BACK_TOP, FRONT_LIGHT, FRONT_DARK] {
            assert!(!out.contains(grey), "{grey} left in {out}");
        }
        assert!(out.contains("#48B9C7"));
        assert_eq!(recolor_svg(r##"<path fill="#123456"/>"##, Rgb(0, 0, 0)), None);
    }
}
