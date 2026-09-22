use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::Deserialize;
use thiserror::Error;
use zones_core::{LayoutDefinition, LayoutKind, NormalizedRect, ZoneId, ZoneSpec};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub outputs: HashMap<String, OutputConfig>,
    #[serde(default)]
    pub layouts: Vec<LayoutConfig>,
    #[serde(default)]
    pub rules: Vec<RuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig {
    pub gap: f64,
    pub geometry_tolerance: f64,
    pub allow_tiled_to_floating: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self { gap: 12.0, geometry_tolerance: 2.0, allow_tiled_to_floating: false }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    pub layout: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutConfig {
    pub name: String,
    pub zones: Vec<ZoneConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneConfig {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RuleConfig {
    pub priority: i32,
    pub app_id: Option<String>,
    pub app_id_regex: Option<String>,
    pub title_regex: Option<String>,
    pub output: Option<String>,
    pub workspace: Option<String>,
    pub layout: Option<String>,
    pub zone: String,
}

#[derive(Debug, Clone)]
pub struct ActiveConfig {
    pub source: Config,
    pub layouts: HashMap<String, LayoutDefinition>,
    pub rules: CompiledRules,
}

#[derive(Debug, Clone)]
pub struct CompiledRules {
    rules: Vec<CompiledRule>,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    source_index: usize,
    priority: i32,
    specificity: usize,
    app_id: Option<String>,
    app_id_regex: Option<Regex>,
    title_regex: Option<Regex>,
    output: Option<String>,
    workspace: Option<String>,
    layout: Option<String>,
    zone: String,
}

#[derive(Debug, Clone, Copy)]
pub struct RuleContext<'a> {
    pub app_id: Option<&'a str>,
    pub title: Option<&'a str>,
    pub output: Option<&'a str>,
    pub workspace: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMatch<'a> {
    pub zone: &'a str,
    pub layout: Option<&'a str>,
    pub priority: i32,
    pub specificity: usize,
    pub source_index: usize,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("unsupported config version {found}; expected {expected}")]
    UnsupportedVersion { found: u32, expected: u32 },
    #[error("general.gap must be finite and >= 0")]
    InvalidGap,
    #[error("general.geometry_tolerance must be finite and >= 0")]
    InvalidTolerance,
    #[error("layout name cannot be empty")]
    EmptyLayoutName,
    #[error("duplicate layout name: {0}")]
    DuplicateLayout(String),
    #[error("layout {0} must contain at least one zone")]
    EmptyLayout(String),
    #[error("layout {layout} contains duplicate zone id: {zone}")]
    DuplicateZone { layout: String, zone: String },
    #[error("invalid geometry for {layout}/{zone}: {message}")]
    InvalidZone { layout: String, zone: String, message: String },
    #[error("output {output} references unknown layout {layout}")]
    UnknownOutputLayout { output: String, layout: String },
    #[error("rule {index} must set zone")]
    EmptyRuleZone { index: usize },
    #[error("rule {index} sets both app_id and app_id_regex; choose one")]
    ConflictingAppMatcher { index: usize },
    #[error("rule {index} has invalid {field} regex: {message}")]
    InvalidRegex { index: usize, field: &'static str, message: String },
    #[error("rule {index} references unknown layout {layout}")]
    UnknownRuleLayout { index: usize, layout: String },
    #[error("rule {index} references unknown zone {zone} in layout {layout}")]
    UnknownRuleZone { index: usize, layout: String, zone: String },
}

impl ActiveConfig {
    /// Parse, validate and compile a complete candidate config. Callers should only
    /// replace their active config after this returns `Ok`, which makes reload atomic.
    pub fn from_toml(input: &str) -> Result<Self, ConfigError> {
        let source: Config = toml::from_str(input)?;
        if source.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: source.version,
                expected: CONFIG_VERSION,
            });
        }
        if !source.general.gap.is_finite() || source.general.gap < 0.0 {
            return Err(ConfigError::InvalidGap);
        }
        if !source.general.geometry_tolerance.is_finite() || source.general.geometry_tolerance < 0.0
        {
            return Err(ConfigError::InvalidTolerance);
        }

        let layouts = compile_layouts(&source.layouts)?;
        for (output, cfg) in &source.outputs {
            if !layouts.contains_key(&cfg.layout) {
                return Err(ConfigError::UnknownOutputLayout {
                    output: output.clone(),
                    layout: cfg.layout.clone(),
                });
            }
        }
        let rules = CompiledRules::compile(&source.rules, &layouts)?;
        Ok(Self { source, layouts, rules })
    }

    pub fn layout_for_output(&self, output: &str) -> Option<&LayoutDefinition> {
        self.source.outputs.get(output).and_then(|cfg| self.layouts.get(&cfg.layout))
    }
}

fn compile_layouts(
    layouts: &[LayoutConfig],
) -> Result<HashMap<String, LayoutDefinition>, ConfigError> {
    let mut compiled = HashMap::new();
    for layout in layouts {
        if layout.name.trim().is_empty() {
            return Err(ConfigError::EmptyLayoutName);
        }
        if layout.zones.is_empty() {
            return Err(ConfigError::EmptyLayout(layout.name.clone()));
        }
        if compiled.contains_key(&layout.name) {
            return Err(ConfigError::DuplicateLayout(layout.name.clone()));
        }

        let mut zone_ids = HashSet::new();
        let mut zones = Vec::with_capacity(layout.zones.len());
        for zone in &layout.zones {
            if !zone_ids.insert(zone.id.clone()) {
                return Err(ConfigError::DuplicateZone {
                    layout: layout.name.clone(),
                    zone: zone.id.clone(),
                });
            }
            let rect =
                NormalizedRect::new(zone.x, zone.y, zone.width, zone.height).map_err(|error| {
                    ConfigError::InvalidZone {
                        layout: layout.name.clone(),
                        zone: zone.id.clone(),
                        message: error.to_string(),
                    }
                })?;
            zones.push(ZoneSpec { id: ZoneId(zone.id.clone()), name: zone.name.clone(), rect });
        }

        compiled.insert(
            layout.name.clone(),
            LayoutDefinition { name: layout.name.clone(), kind: LayoutKind::Rectangles { zones } },
        );
    }
    Ok(compiled)
}

impl CompiledRules {
    fn compile(
        rules: &[RuleConfig],
        layouts: &HashMap<String, LayoutDefinition>,
    ) -> Result<Self, ConfigError> {
        let mut out = Vec::with_capacity(rules.len());
        for (index, rule) in rules.iter().enumerate() {
            if rule.zone.trim().is_empty() {
                return Err(ConfigError::EmptyRuleZone { index });
            }
            if rule.app_id.is_some() && rule.app_id_regex.is_some() {
                return Err(ConfigError::ConflictingAppMatcher { index });
            }
            if let Some(layout_name) = &rule.layout {
                let layout = layouts.get(layout_name).ok_or_else(|| {
                    ConfigError::UnknownRuleLayout { index, layout: layout_name.clone() }
                })?;
                if !layout_has_zone(layout, &rule.zone) {
                    return Err(ConfigError::UnknownRuleZone {
                        index,
                        layout: layout_name.clone(),
                        zone: rule.zone.clone(),
                    });
                }
            }

            let app_id_regex = compile_regex(index, "app_id_regex", rule.app_id_regex.as_deref())?;
            let title_regex = compile_regex(index, "title_regex", rule.title_regex.as_deref())?;
            let specificity = [
                rule.app_id.is_some() || app_id_regex.is_some(),
                title_regex.is_some(),
                rule.output.is_some(),
                rule.workspace.is_some(),
            ]
            .into_iter()
            .filter(|set| *set)
            .count();

            out.push(CompiledRule {
                source_index: index,
                priority: rule.priority,
                specificity,
                app_id: rule.app_id.clone(),
                app_id_regex,
                title_regex,
                output: rule.output.clone(),
                workspace: rule.workspace.clone(),
                layout: rule.layout.clone(),
                zone: rule.zone.clone(),
            });
        }
        Ok(Self { rules: out })
    }

    pub fn best_match<'a>(&'a self, context: RuleContext<'_>) -> Option<RuleMatch<'a>> {
        self.rules
            .iter()
            .filter(|rule| rule.matches(context))
            .max_by(|a, b| compare_rule_priority(a, b))
            .map(|rule| RuleMatch {
                zone: &rule.zone,
                layout: rule.layout.as_deref(),
                priority: rule.priority,
                specificity: rule.specificity,
                source_index: rule.source_index,
            })
    }
}

impl CompiledRule {
    fn matches(&self, context: RuleContext<'_>) -> bool {
        if let Some(expected) = self.app_id.as_deref() {
            if context.app_id != Some(expected) {
                return false;
            }
        }
        if let Some(regex) = &self.app_id_regex {
            if !context.app_id.is_some_and(|value| regex.is_match(value)) {
                return false;
            }
        }
        if let Some(regex) = &self.title_regex {
            if !context.title.is_some_and(|value| regex.is_match(value)) {
                return false;
            }
        }
        if let Some(expected) = self.output.as_deref() {
            if context.output != Some(expected) {
                return false;
            }
        }
        if let Some(expected) = self.workspace.as_deref() {
            if context.workspace != Some(expected) {
                return false;
            }
        }
        true
    }
}

fn compare_rule_priority(a: &CompiledRule, b: &CompiledRule) -> Ordering {
    a.priority
        .cmp(&b.priority)
        .then_with(|| a.specificity.cmp(&b.specificity))
        // Earlier file order wins, so invert source index for max_by().
        .then_with(|| b.source_index.cmp(&a.source_index))
}

fn compile_regex(
    index: usize,
    field: &'static str,
    value: Option<&str>,
) -> Result<Option<Regex>, ConfigError> {
    value
        .map(|pattern| {
            Regex::new(pattern).map_err(|error| ConfigError::InvalidRegex {
                index,
                field,
                message: error.to_string(),
            })
        })
        .transpose()
}

fn layout_has_zone(layout: &LayoutDefinition, zone: &str) -> bool {
    match &layout.kind {
        LayoutKind::Rectangles { zones } => {
            zones.iter().any(|candidate| candidate.id.0 == zone || candidate.name == zone)
        }
        LayoutKind::SplitTree { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
version = 1

[general]
gap = 10
geometry_tolerance = 2

[outputs."eDP-1"]
layout = "coding"

[[layouts]]
name = "coding"

[[layouts.zones]]
id = "editor"
name = "Editor"
x = 0.0
y = 0.0
width = 0.68
height = 1.0

[[layouts.zones]]
id = "terminal"
name = "Terminal"
x = 0.68
y = 0.0
width = 0.32
height = 1.0

[[rules]]
priority = 10
app_id_regex = "^firefox$"
title_regex = "YouTube"
layout = "coding"
zone = "editor"
"#;

    #[test]
    fn valid_config_compiles_before_activation() {
        let config = ActiveConfig::from_toml(BASE).unwrap();
        assert_eq!(config.source.version, 1);
        assert_eq!(config.layout_for_output("eDP-1").unwrap().name, "coding");
    }

    #[test]
    fn malformed_zone_is_rejected() {
        let bad = BASE.replace("width = 0.68", "width = 1.68");
        assert!(matches!(ActiveConfig::from_toml(&bad), Err(ConfigError::InvalidZone { .. })));
    }

    #[test]
    fn invalid_regex_is_rejected_at_activation_not_event_time() {
        let bad = BASE.replace("^firefox$", "(");
        assert!(matches!(ActiveConfig::from_toml(&bad), Err(ConfigError::InvalidRegex { .. })));
    }

    #[test]
    fn priority_then_specificity_then_file_order_is_deterministic() {
        let input = r#"
version = 1

[[layouts]]
name = "main"
[[layouts.zones]]
id = "generic"
name = "Generic"
x = 0.0
y = 0.0
width = 0.5
height = 1.0
[[layouts.zones]]
id = "specific"
name = "Specific"
x = 0.5
y = 0.0
width = 0.5
height = 1.0

[[rules]]
priority = 5
app_id_regex = "firefox"
layout = "main"
zone = "generic"

[[rules]]
priority = 5
app_id_regex = "firefox"
title_regex = "Docs"
layout = "main"
zone = "specific"
"#;
        let config = ActiveConfig::from_toml(input).unwrap();
        let matched = config
            .rules
            .best_match(RuleContext {
                app_id: Some("firefox"),
                title: Some("Docs - Mozilla"),
                output: None,
                workspace: None,
            })
            .unwrap();
        assert_eq!(matched.zone, "specific");
        assert_eq!(matched.specificity, 2);
    }

    #[test]
    fn unknown_fields_fail_closed_instead_of_being_silently_ignored() {
        let bad = BASE.replace("gap = 10", "gap = 10\nmagic = true");
        assert!(matches!(ActiveConfig::from_toml(&bad), Err(ConfigError::Parse(_))));
    }
}
