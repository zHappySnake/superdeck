// SPDX-License-Identifier: GPL-3.0-or-later

//! `superdeck-cli run`: drive a device from its profiles, without the app.
//!
//! Glues the pieces together: key presses from `superdeck-device` go to a
//! `superdeck-core` [`Deck`], and its effects are carried out here, with keys
//! drawn by `superdeck-render`. There's no plugin host yet, so plugin events
//! are printed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use superdeck_core::deck::{FaceImage, PluginEvent, PluginEventKind};
use superdeck_core::model::FontStyle;
use superdeck_core::{
    ConfigDir, Deck, DeckSettings, DeviceSettings, Effect, KeyFace, Layout, Position, Profile,
    ProfileId,
};
use superdeck_device::image::{DynamicImage, Rgb, RgbImage};
use superdeck_device::{Device, DeviceInfo, KeyEvent, Spec, encode_key_image};
use superdeck_render::{KeyImage, Rendered, Renderer, TitleAlignment, TitleStyle};

/// Options of the `run` command.
#[derive(clap::Args)]
pub struct RunArgs {
    /// Configuration directory. Defaults to `$XDG_CONFIG_HOME/superdeck`.
    #[arg(long, value_name = "DIR")]
    config: Option<PathBuf>,
}

/// An animated key: its encoded frames, and which one shows until when.
struct Animation {
    frames: Vec<(Vec<u8>, Duration)>,
    current: usize,
    next_at: Instant,
}

/// Everything `run` keeps while the device runs.
struct Runner<'a> {
    spec: &'static Spec,
    serial: String,
    device: &'a mut Device,
    config: ConfigDir,
    renderer: Renderer,
    /// Loaded images, so they're decoded once.
    images: HashMap<FaceImage, Option<KeyImage>>,
    animations: HashMap<u8, Animation>,
    sleeping: bool,
    deck: Deck,
}

pub fn run(info: &DeviceInfo, device: &mut Device, args: &RunArgs) -> Result<()> {
    let config = match &args.config {
        Some(dir) => ConfigDir::new(dir),
        None => ConfigDir::from_env().context("no home directory; pass --config")?,
    };
    let spec = info.model.spec();
    let serial = device.serial_number()?;
    let layout = Layout {
        columns: spec.columns,
        rows: spec.rows,
    };

    let mut devices = config.load_devices()?.value;
    let settings = if let Some(settings) = devices.devices.get(&serial) {
        settings.clone()
    } else {
        // A device seen for the first time gets an empty "Default" profile.
        let profile = Profile::new("Default", layout);
        config.save_profile(&profile)?;
        let settings = DeviceSettings::new(vec![profile.id]);
        devices.devices.insert(serial.clone(), settings.clone());
        config.save_devices(&devices)?;
        eprintln!(
            "New device: created a Default profile in {}",
            config.root().display()
        );
        settings
    };
    let start = settings
        .start_profile()
        .context("this device has no profiles in devices.json")?;
    let profile = load_profile(&config, start, layout)?;
    eprintln!(
        "Running profile {:?} on the {} ({}); Ctrl+C to stop.",
        profile.name, info.model, serial
    );

    let deck_settings = DeckSettings {
        brightness: settings.brightness,
        sleep_after: settings
            .sleep_after_minutes
            .map(|minutes| Duration::from_secs(u64::from(minutes) * 60)),
    };
    let mut runner = Runner {
        spec,
        serial,
        device,
        config,
        renderer: Renderer::new(),
        images: HashMap::new(),
        animations: HashMap::new(),
        sleeping: false,
        deck: Deck::new(profile, deck_settings, 0, Instant::now()),
    };
    let effects = runner.deck.start();
    runner.apply(effects)?;
    runner.run()
}

fn load_profile(config: &ConfigDir, id: ProfileId, layout: Layout) -> Result<Profile> {
    let profile = config
        .load_profile(id)
        .with_context(|| format!("could not load profile {id}"))?
        .value;
    if profile.layout != layout {
        bail!(
            "profile {:?} is for a {} layout, but this device is {}",
            profile.name,
            profile.layout,
            layout
        );
    }
    Ok(profile)
}

impl Runner<'_> {
    fn run(&mut self) -> Result<()> {
        loop {
            let now = Instant::now();
            let deadline = [self.deck.next_deadline(), self.next_frame()]
                .into_iter()
                .flatten()
                .min();
            let timeout = deadline.map(|deadline| deadline.saturating_duration_since(now));
            let events = self.device.read_key_events(timeout)?;
            let now = Instant::now();
            for event in events {
                let effects = match event {
                    KeyEvent::Down(key) => self.deck.key_down(self.position(key), now),
                    KeyEvent::Up(key) => self.deck.key_up(self.position(key), now),
                };
                self.apply(effects)?;
            }
            let effects = self.deck.poll(now);
            self.apply(effects)?;
            self.animate(now)?;
        }
    }

    fn position(&self, key: u8) -> Position {
        Position::new(key % self.spec.columns, key / self.spec.columns)
    }

    fn key(&self, position: Position) -> u8 {
        position.row * self.spec.columns + position.column
    }

    fn apply(&mut self, effects: Vec<Effect>) -> Result<()> {
        for effect in effects {
            match effect {
                Effect::Draw { position, face } => self.draw(position, face.as_ref())?,
                Effect::Brightness { percent, save } => {
                    self.device.set_brightness(percent)?;
                    if save {
                        let mut devices = self.config.load_devices()?.value;
                        if let Some(settings) = devices.devices.get_mut(&self.serial) {
                            settings.brightness = percent;
                            self.config.save_devices(&devices)?;
                        }
                    }
                }
                Effect::Sleep => {
                    self.sleeping = true;
                    self.device.set_brightness(0)?;
                }
                Effect::Wake => self.sleeping = false,
                Effect::Plugin(event) => print_plugin_event(&event),
                Effect::SwitchProfile { profile, page } => self.switch_profile(profile, page)?,
                Effect::ProfileChanged => self.config.save_profile(self.deck.profile())?,
            }
        }
        Ok(())
    }

    fn switch_profile(&mut self, id: ProfileId, page: usize) -> Result<()> {
        let layout = self.deck.profile().layout;
        let profile = match load_profile(&self.config, id, layout) {
            Ok(profile) => profile,
            Err(error) => {
                eprintln!("Not switching profile: {error:#}");
                return Ok(());
            }
        };
        eprintln!("Switching to profile {:?}", profile.name);
        let effects = self.deck.stop();
        self.apply(effects)?;
        let devices = self.config.load_devices()?.value;
        let settings = devices.devices.get(&self.serial);
        let deck_settings = DeckSettings {
            brightness: settings.map_or(superdeck_core::devices::DEFAULT_BRIGHTNESS, |s| {
                s.brightness
            }),
            sleep_after: settings
                .and_then(|s| s.sleep_after_minutes)
                .map(|minutes| Duration::from_secs(u64::from(minutes) * 60)),
        };
        self.deck = Deck::new(profile, deck_settings, page, Instant::now());
        let effects = self.deck.start();
        self.apply(effects)
    }

    fn draw(&mut self, position: Position, face: Option<&KeyFace>) -> Result<()> {
        let key = self.key(position);
        self.animations.remove(&key);
        let Some(face) = face else {
            return Ok(self.device.set_key_image_data(
                key,
                &self.encode(&RgbImage::new(self.spec.key_size, self.spec.key_size))?,
            )?);
        };
        let image = face.image.as_ref().and_then(|image| self.image(image));
        let style = TitleStyle {
            visible: face.show_title,
            font_family: face.font_family.clone(),
            #[expect(clippy::cast_possible_truncation, reason = "at most 144")]
            font_size: face.font_size as f32,
            bold: matches!(face.font_style, FontStyle::Bold | FontStyle::BoldItalic),
            italic: matches!(face.font_style, FontStyle::Italic | FontStyle::BoldItalic),
            underline: face.font_underline,
            alignment: match face.title_alignment {
                superdeck_core::model::TitleAlignment::Top => TitleAlignment::Top,
                superdeck_core::model::TitleAlignment::Middle => TitleAlignment::Middle,
                superdeck_core::model::TitleAlignment::Bottom => TitleAlignment::Bottom,
            },
            color: Rgb(face.title_color.0),
        };
        match self
            .renderer
            .render(image.as_ref(), &face.title, &style, self.spec.key_size)
        {
            Rendered::Still(image) => {
                let data = self.encode(&image)?;
                self.device.set_key_image_data(key, &data)?;
            }
            Rendered::Animated(frames) => {
                let frames = frames
                    .iter()
                    .map(|frame| Ok((self.encode(&frame.image)?, frame.delay)))
                    .collect::<Result<Vec<_>>>()?;
                self.device.set_key_image_data(key, &frames[0].0)?;
                let next_at = Instant::now() + frames[0].1;
                self.animations.insert(
                    key,
                    Animation {
                        frames,
                        current: 0,
                        next_at,
                    },
                );
            }
        }
        Ok(())
    }

    /// Loads a face's image once; `None` if it can't be loaded.
    fn image(&mut self, image: &FaceImage) -> Option<KeyImage> {
        if let Some(loaded) = self.images.get(image) {
            return loaded.clone();
        }
        let loaded = match image {
            FaceImage::Builtin(icon) => self.renderer.load_image(icon.svg()).map_err(Into::into),
            FaceImage::DataUrl(url) => self.renderer.load_data_url(url).map_err(Into::into),
            FaceImage::Profile(path) => {
                let file = self.config.image_file(self.deck.profile().id, path);
                std::fs::read(&file)
                    .with_context(|| format!("could not read {}", file.display()))
                    .and_then(|data| Ok(self.renderer.load_image(&data)?))
            }
        };
        let loaded = loaded
            .inspect_err(|error: &anyhow::Error| eprintln!("Image not shown: {error:#}"))
            .ok();
        self.images.insert(image.clone(), loaded.clone());
        loaded
    }

    fn encode(&self, image: &RgbImage) -> Result<Vec<u8>> {
        Ok(encode_key_image(
            self.spec,
            &DynamicImage::ImageRgb8(image.clone()),
        )?)
    }

    fn next_frame(&self) -> Option<Instant> {
        if self.sleeping {
            return None;
        }
        self.animations.values().map(|a| a.next_at).min()
    }

    fn animate(&mut self, now: Instant) -> Result<()> {
        if self.sleeping {
            return Ok(());
        }
        for (&key, animation) in &mut self.animations {
            if animation.next_at <= now {
                animation.current = (animation.current + 1) % animation.frames.len();
                let (data, delay) = &animation.frames[animation.current];
                self.device.set_key_image_data(key, data)?;
                animation.next_at = now + *delay;
            }
        }
        Ok(())
    }
}

fn print_plugin_event(event: &PluginEvent) {
    let kind = match event.kind {
        PluginEventKind::KeyDown => "keyDown",
        PluginEventKind::KeyUp => "keyUp",
        PluginEventKind::WillAppear => "willAppear",
        PluginEventKind::WillDisappear => "willDisappear",
    };
    let at = event
        .position
        .map_or_else(|| "in a Multi Action".to_owned(), |p| format!("at {p}"));
    println!(
        "plugin event: {kind:<13} {} {at} (state {}, settings {})",
        event.action,
        event.state,
        serde_json::Value::Object(event.settings.clone())
    );
}
