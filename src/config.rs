use color_eyre::Result;
use gpui::{Hsla, Window, black, px, size, white};
use indexmap::IndexMap;
use notify::Watcher;
use serde::{Deserialize, Deserializer, de};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

#[cfg(target_os = "linux")]
pub type Key = evdev::KeyCode;
#[cfg(not(target_os = "linux"))]
pub type Key = rdev::Key;

#[derive(Debug)]
pub struct Input {
    pub key: Key,
    pub color: Hsla,
    pub text: String,
    pub width: u8,
}

impl Input {
    fn hitbox_width(&self, size: f32) -> f32 {
        size * self.width as f32
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(deserialize_with = "deserialize_keys")]
    pub keys: Vec<Input>,
    pub speed: f32,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_height")]
    pub height: f32,
    pub size: f32,
    #[serde(default = "default_bg")]
    pub bg: Hsla,
    #[serde(default)]
    pub counter: bool,
}

fn default_gap() -> f32 {
    18.0
}

fn default_height() -> f32 {
    700.0
}

fn default_bg() -> Hsla {
    black()
}

const ALIASES: &[(&str, &str, &str)] = &[
    ("esc", "Escape", "ESC"),
    ("escape", "Escape", "ESC"),
    ("enter", "Return", "ENTER"),
    ("return", "Return", "ENTER"),
    ("left", "LeftArrow", "LEFT"),
    ("right", "RightArrow", "RIGHT"),
    ("up", "UpArrow", "UP"),
    ("down", "DownArrow", "DOWN"),
    ("altgr", "AltGr", "RIGHTALT"),
    ("printscreen", "PrintScreen", "SYSRQ"),
    ("semicolon", "SemiColon", "SEMICOLON"),
    ("quote", "Quote", "APOSTROPHE"),
    ("period", "Dot", "DOT"),
];

pub fn resolve_key_name(name: &str) -> (String, String) {
    let find_aliases = |name, normalized| {
        ALIASES
            .iter()
            .find(|(alias, r, ev)| normalized == *alias || name == *r || name == *ev)
    };

    let normalized = name
        .trim()
        .trim_prefix("KEY_")
        .trim_prefix("Key")
        .to_lowercase();

    if let Some((_, rdev, evdev)) = find_aliases(name, &normalized) {
        return (rdev.to_string(), format!("KEY_{evdev}"));
    }

    let (rdev, evdev) = match normalized.as_bytes() {
        [letter] if letter.is_ascii_alphabetic() => {
            let letter = normalized.to_ascii_uppercase();
            (format!("Key{letter}"), normalized.to_uppercase())
        }
        [digit] if digit.is_ascii_digit() => (format!("Num{normalized}"), normalized.to_string()),
        _ => match normalized.strip_prefix("f") {
            Some(function) => (format!("F{function}"), format!("F{function}")),
            None => {
                let mut chars = normalized.chars();
                let rdev = chars
                    .next()
                    .map(|first| {
                        first.to_ascii_uppercase().to_string()
                            + &chars.as_str().to_ascii_lowercase()
                    })
                    .unwrap_or_default();

                let evdev = normalized.to_uppercase();

                (rdev, evdev)
            }
        },
    };

    (rdev, format!("KEY_{evdev}"))
}

#[cfg(target_os = "linux")]
fn parse_config_key(name: &str) -> Option<Key> {
    use std::str::FromStr;

    let (_, evdev) = resolve_key_name(name);
    evdev::KeyCode::from_str(&evdev).ok()
}

#[cfg(not(target_os = "linux"))]
fn parse_config_key(name: &str) -> Option<Key> {
    use serde::de::{
        IntoDeserializer,
        value::{Error, StringDeserializer},
    };

    let (rdev, _) = resolve_key_name(name);
    let de: StringDeserializer<Error> = rdev.into_deserializer();
    rdev::Key::deserialize(de).ok()
}

impl Default for Config {
    fn default() -> Self {
        let keys = ["Z", "X"]
            .into_iter()
            .map(|key| Input {
                key: parse_config_key(key).expect("default keys are valid"),
                color: white(),
                text: key.into(),
                width: 1,
            })
            .collect();

        Self {
            keys,
            speed: 200.0,
            gap: default_gap(),
            height: default_height(),
            size: 44.0,
            bg: black(),
            counter: true,
        }
    }
}

fn deserialize_keys<'de, D>(deserializer: D) -> Result<Vec<Input>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Data {
        color: Hsla,
        text: Option<String>,
        #[serde(default)]
        width: u8,
    }

    IndexMap::<String, Data>::deserialize(deserializer)?
        .into_iter()
        .map(|(name, Data { color, text, width })| {
            let key = parse_config_key(&name).ok_or_else(|| {
                de::Error::custom(format!(
                    "unknown key `{name}` (try a capitalized name such as `KeyS`, `Space`, or `Quote`)"
                ))
            })?;

            Ok(Input {
                key,
                color,
                text: text.unwrap_or(name),
                width: width.max(1),
            })
        })
        .collect()
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&contents)?)
    }

    pub fn total_width(&self) -> f32 {
        self.keys
            .iter()
            .map(|key| key.hitbox_width(self.size) + self.gap)
            .sum::<f32>()
            + self.gap
    }

    pub fn lane_x(&self, lane: usize) -> f32 {
        self.keys[..lane]
            .iter()
            .map(|key| key.hitbox_width(self.size) + self.gap)
            .sum::<f32>()
            + self.gap
    }

    pub fn lane_width(&self, lane: usize) -> f32 {
        self.keys[lane].hitbox_width(self.size)
    }
}

pub fn is_toml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
}

pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn latest_toml(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| is_toml(path) && path.is_file())
        .filter_map(|path| {
            let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| canonical(&path))
}

pub struct ConfigStore {
    config: Config,
    error: Option<String>,
    watcher: Option<notify::RecommendedWatcher>,
    pub rx: mpsc::Receiver<notify::Result<notify::Event>>,
    pub dir: PathBuf,
    pub pending: bool,
}

impl ConfigStore {
    pub fn new() -> Self {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let (tx, rx) = mpsc::channel();

        let watcher = notify::recommended_watcher(tx).ok().map(|mut w| {
            let _ = w.watch(&dir, notify::RecursiveMode::NonRecursive);
            w
        });

        let mut this = Self {
            config: Config::default(),
            error: None,
            watcher,
            rx,
            dir: dir.clone(),
            pending: false,
        };

        if let Some(path) = latest_toml(&dir) {
            this.load(&path);
        }

        this
    }

    pub fn get(&self) -> &Config {
        &self.config
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn load(&mut self, path: &Path) {
        if !is_toml(path) {
            let name = path.file_name().unwrap_or(path.as_os_str());
            self.error = Some(format!("{name:?} is not a TOML file"));
            return;
        }

        match Config::load(path) {
            Ok(config) => {
                self.config = config;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
            }
        }

        self.dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(PathBuf::from("."));

        self.update_watcher();
        self.pending = true;
    }

    pub fn apply(&mut self, window: &mut Window) {
        window.resize(size(px(self.config.total_width()), px(self.config.height)));
        self.pending = false;
    }

    fn update_watcher(&mut self) {
        let Some(ref mut watcher) = self.watcher else {
            return;
        };

        let _ = watcher.unwatch(&self.dir);
        let _ = watcher.watch(&self.dir, notify::RecursiveMode::NonRecursive);
    }
}
