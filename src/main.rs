use std::{collections::HashSet, path::Path, sync::mpsc, time::Instant};

use color_eyre::Result;
use gpui::{
    App, Application, Bounds, Context, Hsla, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size, transparent_black,
};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, de};

#[cfg(target_os = "linux")]
type Key = evdev::KeyCode;
#[cfg(not(target_os = "linux"))]
type Key = rdev::Key;

#[derive(Debug)]
struct Input {
    key: Key,
    color: Hsla,
    text: String,
    width: u8,
}

impl Input {
    fn hitbox_width(&self, size: f32) -> f32 {
        size * self.width as f32
    }
}

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(deserialize_with = "deserialize_keys")]
    keys: Vec<Input>,
    speed: f32,
    padding: f32,
    #[serde(default = "default_gap")]
    gap: f32,
    #[serde(default = "default_height")]
    height: f32,
    size: f32,
    #[serde(default = "default_bg")]
    bg: Hsla,
}

fn default_gap() -> f32 {
    18.0
}

fn default_height() -> f32 {
    700.0
}

fn default_bg() -> Hsla {
    gpui::opaque_grey(0.3, 1.0)
}

#[cfg(not(target_os = "linux"))]
fn parse_config_key(name: &str) -> Option<Key> {
    use serde::de::{
        IntoDeserializer,
        value::{Error, StrDeserializer},
    };

    let de: StrDeserializer<Error> = name.into_deserializer();
    rdev::Key::deserialize(de).ok()
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
    fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&contents)?)
    }

    fn total_width(&self) -> f32 {
        self.keys
            .iter()
            .map(|key| key.hitbox_width(self.size) + self.gap)
            .sum::<f32>()
            .max(self.gap)
    }

    fn lane_x(&self, lane: usize) -> f32 {
        self.keys[..lane]
            .iter()
            .map(|key| key.hitbox_width(self.size) + self.gap)
            .sum::<f32>()
            + self.padding
    }

    fn lane_width(&self, lane: usize) -> f32 {
        self.keys[lane].hitbox_width(self.size)
    }
}

enum Message {
    Pressed { key: Key, at: Instant },
    Released { key: Key, at: Instant },
}

const FLOOR_INSET: f32 = 80.0;

struct KeyVisualizer {
    config: Config,
    rx: mpsc::Receiver<Message>,
    held: HashSet<Key>,
}

impl Render for KeyVisualizer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = f32::from(window.viewport_size().height);
        let now = Instant::now();

        let labels = self.render_labels(height);

        div().flex().size_full().bg(self.config.bg).children(labels)
    }
}

impl KeyVisualizer {
    fn new(config: Config, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = mpsc::channel::<Message>();

        Self {
            config,
            rx,
            held: HashSet::new(),
        }
    }

    fn render_labels(&self, height: f32) -> Vec<impl IntoElement> {
        self.config
            .keys
            .iter()
            .enumerate()
            .map(
                |(
                    lane,
                    Input {
                        key, color, text, ..
                    },
                )| {
                    let (bg, fg) = match self.held.contains(key) {
                        true => (*color, transparent_black()),
                        false => (transparent_black(), *color),
                    };

                    div()
                        .absolute()
                        .left(px(self.config.lane_x(lane)))
                        .top(px(height - FLOOR_INSET))
                        .w(px(self.config.lane_width(lane)))
                        .h(px(self.config.size))
                        .border_2()
                        .border_color(*color)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .bg(bg)
                        .text_color(fg)
                        .child(text.clone())
                },
            )
            .collect()
    }
}

fn main() {
    let config = Config::load("config.toml");
    println!("{config:?}");
    let config = config.unwrap();

    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(config.total_width()), px(config.height)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Key Overlay".into()),
                    ..Default::default()
                }),
                is_resizable: false,
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| KeyVisualizer::new(config, cx)),
        )
        .unwrap();
    });
}
