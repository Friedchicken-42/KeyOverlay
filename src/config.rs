use color_eyre::Result;
use gpui::{Hsla, black, white};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, de};
use std::path::{Path, PathBuf};

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
    pub padding: f32,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_height")]
    pub height: f32,
    pub size: f32,
    #[serde(default = "default_bg")]
    pub bg: Hsla,
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
    ("esc", "Escape", "KEY_ESC"),
    ("escape", "Escape", "KEY_ESC"),
    ("enter", "Return", "KEY_ENTER"),
    ("return", "Return", "KEY_ENTER"),
    ("left", "LeftArrow", "KEY_LEFT"),
    ("right", "RightArrow", "KEY_RIGHT"),
    ("up", "UpArrow", "KEY_UP"),
    ("down", "DownArrow", "KEY_DOWN"),
    ("altgr", "AltGr", "KEY_RIGHTALT"),
    ("printscreen", "PrintScreen", "KEY_SYSRQ"),
    ("semicolon", "SemiColon", "SEMICOLON"),
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
        return (rdev.to_string(), evdev.to_string());
    }

    let (rdev, evdev) = match normalized.as_bytes() {
        [letter] if letter.is_ascii_alphabetic() => {
            let letter = normalized.to_ascii_uppercase();
            (format!("Key{letter}"), normalized.to_string())
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
    evdev::KeyCode::from_str(evdev).ok()
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
            padding: default_gap(),
            gap: default_gap(),
            height: default_height(),
            size: 44.0,
            bg: black(),
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
            .max(self.gap)
    }

    pub fn lane_x(&self, lane: usize) -> f32 {
        self.keys[..lane]
            .iter()
            .map(|key| key.hitbox_width(self.size) + self.gap)
            .sum::<f32>()
            + self.padding
    }

    pub fn lane_width(&self, lane: usize) -> f32 {
        self.keys[lane].hitbox_width(self.size)
    }
}

fn is_toml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn latest_toml(dir: &Path) -> Option<PathBuf> {
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
}

impl ConfigStore {
    pub fn new() -> Self {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let path = latest_toml(&dir);

        let (config, error) = match path {
            Some(path) => match Config::load(&path) {
                Ok(config) => (config, None),
                Err(err) => (Config::default(), Some(format!("{err:#}"))),
            },
            None => (Config::default(), None),
        };

        Self { config, error }
    }

    pub fn get(&self) -> &Config {
        &self.config
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}
