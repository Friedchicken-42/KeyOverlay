#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod config;

use std::{
    collections::{HashMap, HashSet},
    sync::mpsc,
    time::{Duration, Instant},
};

use color_eyre::Result;
use gpui::{
    App, Application, Bounds, Context, ExternalPaths, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, linear_color_stop, linear_gradient, prelude::*, px, rgb, size, white,
};
use notify::EventKind;

use crate::config::{ConfigStore, Input, Key, canonical, is_toml};

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
    config: ConfigStore,
    rx: mpsc::Receiver<Message>,
    segments: Vec<Segment>,
    held: HashSet<Key>,
    counters: HashMap<Key, usize>,
}

impl Render for KeyVisualizer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = f32::from(window.viewport_size().height);
        let now = Instant::now();

        self.drain_segments(now, height);

        if self.config.pending {
            self.config.apply(window);
        }

        let config = self.config.get();

        let labels = self.render_labels(height);
        let segments = self.render_segments(now, height);
        let error = self.render_error();
        let counters = match config.counter {
            true => self.render_counters(height),
            false => vec![],
        };

        let fade = div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(100.0))
            .bg(linear_gradient(
                180.0,
                linear_color_stop(config.bg, 0.0),
                linear_color_stop(config.bg.opacity(0.0), 1.0),
            ));

        div()
            .flex()
            .size_full()
            .bg(config.bg)
            .children(labels)
            .children(segments)
            .children(counters)
            .child(fade)
            .children(error)
            .on_drop(cx.listener(Self::drop_config))
    }
}

impl KeyVisualizer {
    fn new(config: ConfigStore, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = mpsc::channel::<Message>();

        Self::spawn_key_listener(cx, tx);
        Self::spawn_update_loop(cx);

        let counters = config.get().keys.iter().map(|k| (k.key, 0)).collect();

        Self {
            config,
            rx,
            segments: Vec::new(),
            held: HashSet::new(),
            counters,
        }
    }

    fn spawn_key_listener(cx: &mut Context<Self>, tx: mpsc::Sender<Message>) {
        cx.background_executor()
            .spawn(async move {
                #[cfg(not(target_os = "linux"))]
                let result = listen_rdev(tx);
                #[cfg(target_os = "linux")]
                let result = listen_evdev(tx);

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
                    this.poll_folder();
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
                    self.counters
                        .entry(key)
                        .and_modify(|v| *v += 1)
                        .or_insert(0);

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

    fn poll_folder(&mut self) {
        while let Ok(event) = self.config.rx.try_recv() {
            let Ok(event) = event else {
                continue;
            };

            let Some(file) = event.paths.into_iter().find(|path| is_toml(path)) else {
                continue;
            };

            if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                self.config.load(&file);
            }
        }
    }

    fn drain_segments(&mut self, now: Instant, height: f32) {
        let speed = self.config.get().speed;
        let off_screen = Duration::from_secs_f32((height / speed).max(1.0) + 1.0);

        self.segments
            .retain(|s| s.end.is_none_or(|end| now.duration_since(end) < off_screen));
    }

    fn render_labels(&self, height: f32) -> Vec<impl IntoElement> {
        let config = self.config.get();

        config
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
                        false => config.bg,
                    };

                    div()
                        .absolute()
                        .left(px(config.lane_x(lane)))
                        .top(px(height - FLOOR_INSET))
                        .w(px(config.lane_width(lane)))
                        .h(px(config.size))
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
        self.config
            .get()
            .keys
            .iter()
            .position(|input| input.key == key)
    }

    fn segment_top(&self, segment_time: Instant, now: Instant) -> f32 {
        now.duration_since(segment_time).as_secs_f32() * self.config.get().speed
    }

    fn render_segments(&self, now: Instant, height: f32) -> Vec<impl IntoElement> {
        let config = self.config.get();

        self.segments
            .iter()
            .filter_map(|segment| {
                let lane = self.lane_for_key(segment.key)?;

                let Input { color, .. } = &config.keys[lane];
                let end = segment.end.unwrap_or(now);

                let top = self.segment_top(segment.start, now);
                let bottom = self.segment_top(end, now) - 2.0;
                let bar_height = (top - bottom).max(MIN_BAR_HEIGHT);

                Some(
                    div()
                        .absolute()
                        .left(px(config.lane_x(lane)))
                        .top(px(height - FLOOR_INSET - top))
                        .w(px(config.lane_width(lane)))
                        .h(px(bar_height))
                        .bg(color.alpha(color.a / 1.618)),
                )
            })
            .collect()
    }

    fn render_error(&self) -> Option<impl IntoElement> {
        self.config.error().map(|msg| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .px_2()
                .py_1()
                .bg(rgb(0xfbbf24))
                .text_color(rgb(0x111827))
                .text_xs()
                .child(format!("Config error: {msg}"))
        })
    }

    fn render_counters(&self, height: f32) -> Vec<impl IntoElement> {
        let config = self.config.get();

        self.counters
            .iter()
            .flat_map(|(key, count)| {
                let lane = config.keys.iter().position(|i| i.key == *key)?;

                Some(
                    div()
                        .absolute()
                        .left(px(config.lane_x(lane)))
                        .top(px(height - FLOOR_INSET + config.size))
                        .w(px(config.lane_width(lane)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(white())
                        .child(count.to_string()),
                )
            })
            .collect()
    }

    fn drop_config(&mut self, paths: &ExternalPaths, window: &mut Window, _cx: &mut Context<Self>) {
        let Some(path) = paths.paths().first() else {
            return;
        };

        let path = canonical(path);
        self.config.load(&path);
        self.config.apply(window);
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

#[cfg(target_os = "linux")]
fn listen_evdev(tx: mpsc::Sender<Message>) -> Result<()> {
    use color_eyre::eyre::Context;
    use evdev::*;

    let mut device = Device::open("/dev/input/event0")
        .context("Error reading /dev/input/event0, try adding yourself to the `input` group")?;

    device.set_nonblocking(true)?;

    loop {
        let Ok(events) = device.fetch_events() else {
            continue;
        };

        for event in events {
            let EventSummary::Key(_, key, _) = event.destructure() else {
                continue;
            };

            let at = Instant::now();

            let msg = match event.value() {
                1 => Message::Pressed { key, at },
                0 => Message::Released { key, at },
                _ => continue,
            };

            let _ = tx.send(msg);
        }
    }
}

fn main() {
    let config = ConfigStore::new();

    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(
            None,
            size(px(config.get().total_width()), px(config.get().height)),
            cx,
        );

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
            move |_, cx| cx.new(|cx| KeyVisualizer::new(config, cx)),
        )
        .unwrap();
    });
}
