//! Bundled catalog plus the user's own layer (edits, additions, favourites, window settings).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const CATALOG_JSON: &str = include_str!("../../data/prompts.json");

#[derive(Deserialize, Clone)]
pub struct Category {
    pub id: String,
    pub label: String,
    pub short: String,
    pub icon: String,
    pub color: String,
}

#[derive(Deserialize)]
struct Source {
    label: String,
}

#[derive(Deserialize)]
struct RawPrompt {
    id: String,
    title: String,
    category: String,
    #[serde(default)]
    sub: String,
    prompt: String,
    #[serde(rename = "promptEn")]
    prompt_en: Option<String>,
    source: Option<Source>,
}

#[derive(Deserialize)]
struct Catalog {
    categories: Vec<Category>,
    prompts: Vec<RawPrompt>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Origin {
    Builtin,
    Edited,
    Added,
}

#[derive(Clone)]
pub struct Item {
    pub id: String,
    pub category: String,
    pub sub: String,
    pub title: String,
    pub body: String,
    pub body_en: Option<String>,
    pub source: String,
    pub origin: Origin,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct EditRec {
    pub title: String,
    pub sub: String,
    pub prompt: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserPrompt {
    pub id: String,
    pub category: String,
    pub sub: String,
    pub title: String,
    pub prompt: String,
}

fn default_opacity() -> f32 {
    0.8
}
fn default_scale() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub edits: HashMap<String, EditRec>,
    #[serde(default)]
    pub added: Vec<UserPrompt>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// Window top-left in physical pixels.
    #[serde(default)]
    pub pos: Option<[i32; 2]>,
    #[serde(default)]
    pub mini: bool,
    #[serde(default = "default_true")]
    pub pinned: bool,
    /// Playful effects: sparks on a hard spin, parts falling when shaken, the ten-click burst.
    #[serde(default = "default_true")]
    pub play: bool,
    /// Window layout the saved position refers to; lets a newer build migrate it.
    #[serde(default)]
    pub layout: u32,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            edits: HashMap::new(),
            added: Vec::new(),
            opacity: default_opacity(),
            scale: default_scale(),
            pos: None,
            mini: false,
            pinned: true,
            play: true,
            layout: 2,
        }
    }
}

fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("KneekuraPromptWheeler").join("state.json"))
}

impl Saved {
    pub fn load() -> Self {
        let mut saved: Saved = state_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        saved.opacity = saved.opacity.clamp(0.3, 1.0);
        saved.scale = saved.scale.clamp(0.7, 1.6);
        saved
    }

    /// Write to a temp file first so a crash mid-write cannot leave a truncated state file.
    pub fn store(&self) {
        let Some(path) = state_path() else { return };
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn load_catalog() -> (Vec<Category>, Vec<Item>) {
    let catalog: Catalog = serde_json::from_str(CATALOG_JSON).expect("bundled data/prompts.json is valid");
    let items = catalog
        .prompts
        .into_iter()
        .map(|p| Item {
            id: p.id,
            category: p.category,
            sub: p.sub,
            title: p.title,
            body: p.prompt,
            body_en: p.prompt_en,
            source: p.source.map(|s| s.label).unwrap_or_default(),
            origin: Origin::Builtin,
        })
        .collect();
    (catalog.categories, items)
}

/// Built-in prompts with the user's edits applied, followed by the user's own prompts.
pub fn merge(categories: &[Category], base: &[Item], saved: &Saved) -> Vec<Item> {
    let mut items: Vec<Item> = base
        .iter()
        .map(|item| match saved.edits.get(&item.id) {
            Some(edit) => Item {
                title: edit.title.clone(),
                sub: edit.sub.clone(),
                body: edit.prompt.clone(),
                origin: Origin::Edited,
                ..item.clone()
            },
            None => item.clone(),
        })
        .collect();
    for p in &saved.added {
        if categories.iter().any(|c| c.id == p.category) {
            items.push(Item {
                id: p.id.clone(),
                category: p.category.clone(),
                sub: p.sub.clone(),
                title: p.title.clone(),
                body: p.prompt.clone(),
                body_en: None,
                source: "自作".to_owned(),
                origin: Origin::Added,
            });
        }
    }
    items
}
