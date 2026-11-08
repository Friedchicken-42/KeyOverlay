use std::{
    collections::HashSet,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

use color_eyre::Result;
use gpui::{
    App, Application, Bounds, Context, Hsla, TitlebarOptions, Window, WindowBounds, WindowOptions,
    black, div, prelude::*, px, size, white,
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

fn resolve_key_name(name: &str) -> (String, String) {
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

struct Segment {
    key: Key,
    start: Instant,
    end: Option<Instant>,
}

enum Message {
    Pressed { key: Key, at: Instant },
    Released { key: Key, at: Instant },
}

const FLOOR_INSET: f32 = 80.0;
const FRAME: Duration = Duration::from_millis(16);
const MIN_BAR_HEIGHT: f32 = 1.0;

struct KeyVisualizer {
    config: Config,
    rx: mpsc::Receiver<Message>,
    segments: Vec<Segment>,
    held: HashSet<Key>,
}

impl Render for KeyVisualizer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = f32::from(window.viewport_size().height);
        let now = Instant::now();

        let labels = self.render_labels(height);
        let segments = self.render_segments(now, height);

        div()
            .flex()
            .size_full()
            .bg(self.config.bg)
            .children(labels)
            .children(segments)
    }
}

impl KeyVisualizer {
    fn new(config: Config, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = mpsc::channel::<Message>();

        Self::spawn_key_listener(cx, tx);
        Self::spawn_update_loop(cx);

        Self {
            config,
            rx,
            segments: Vec::new(),
            held: HashSet::new(),
        }
    }

    fn spawn_key_listener(cx: &mut Context<Self>, tx: mpsc::Sender<Message>) {
        cx.background_executor()
            .spawn(async move {
                #[cfg(not(target_os = "linux"))]
                let result = listen_rdev(tx);

                if let Err(e) = result {
                    eprintln!("Keyboard listener failed: {e:?}");
                }
            })
            .detach();
    }

    fn spawn_update_loop(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;

                let result = this.update(cx, |this, cx| {
                    this.process_keyevents();
                    cx.notify();
                });

                if result.is_err() {
                    break;
                }
            }
        })
        .detach()
    }

    fn process_keyevents(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Message::Pressed { key, at } if self.held.insert(key) => {
                    self.segments.push(Segment {
                        key,
                        start: at,
                        end: None,
                    });
                }
                Message::Released { key, at } if self.held.remove(&key) => {
                    if let Some(segment) = self
                        .segments
                        .iter_mut()
                        .rev()
                        .find(|s| s.key == key && s.end.is_none())
                    {
                        segment.end = Some(at);
                    }
                }
                _ => {} // Ignore duplicated events
            }
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
                    let color_press = color.alpha(color.a / 1.618);

                    let bg = match self.held.contains(key) {
                        true => color_press,
                        false => self.config.bg,
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
                        .text_color(white())
                        .child(text.clone())
                },
            )
            .collect()
    }

    fn lane_for_key(&self, key: Key) -> Option<usize> {
        self.config.keys.iter().position(|input| input.key == key)
    }

    fn segment_top(&self, segment_time: Instant, now: Instant) -> f32 {
        now.duration_since(segment_time).as_secs_f32() * self.config.speed
    }

    fn render_segments(&self, now: Instant, height: f32) -> Vec<impl IntoElement> {
        self.segments
            .iter()
            .filter_map(|segment| {
                let lane = self.lane_for_key(segment.key)?;

                let Input { color, .. } = &self.config.keys[lane];
                let end = segment.end.unwrap_or(now);

                let top = self.segment_top(segment.start, now);
                let bottom = self.segment_top(end, now) - 2.0;
                let bar_height = (top - bottom).max(MIN_BAR_HEIGHT);

                Some(
                    div()
                        .absolute()
                        .left(px(self.config.lane_x(lane)))
                        .top(px(height - FLOOR_INSET - top))
                        .w(px(self.config.lane_width(lane)))
                        .h(px(bar_height))
                        .bg(color.alpha(color.a / 1.618)),
                )
            })
            .collect()
    }
}

#[cfg(not(target_os = "linux"))]
fn listen_rdev(tx: mpsc::Sender<Message>) -> Result<(), rdev::ListenError> {
    rdev::listen(move |event| {
        use rdev::EventType::*;

        let at = Instant::now();

        let msg = match event.event_type {
            KeyPress(key) => Some(Message::Pressed { key, at }),
            KeyRelease(key) => Some(Message::Released { key, at }),
            _ => None,
        };

        if let Some(msg) = msg {
            let _ = tx.send(msg);
        }
    })
}

fn main() {
    let config = Config::load("config.toml");
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
