// SPDX-License-Identifier: GPL-3.0-or-later

//! `superdeck-cli`: drive a Stream Deck by hand, to test the device layer.

mod plugins;
mod run;

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use superdeck_device::image::{self, DynamicImage, Rgb, RgbImage};
use superdeck_device::{
    Device, DeviceChange, DeviceInfo, DeviceManager, KeyEvent, Recorder, encode_key_image,
};
use superdeck_render::{Rendered, Renderer, TitleAlignment, TitleStyle};

#[derive(Parser)]
#[command(
    version,
    about = "Drive a Stream Deck by hand, to test SuperDeck's device layer"
)]
struct Cli {
    /// Device to use, by serial number or path (`/dev/hidrawN`). Defaults to
    /// the first one found.
    #[arg(short, long, global = true)]
    device: Option<String>,

    /// Write all HID traffic to a capture file, for bug reports and protocol
    /// tests.
    #[arg(long, global = true, value_name = "FILE")]
    record: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List connected devices.
    List,
    /// Show model, serial number, firmware version, and key layout.
    Info,
    /// Print devices as they're plugged in and unplugged, until Ctrl+C.
    Watch,
    /// Print key presses and releases, until Ctrl+C.
    Events,
    /// Set the backlight brightness.
    Brightness {
        /// Percentage, 0 to 100.
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        percent: u8,
    },
    /// Fill keys with a solid color.
    Fill {
        /// `#rrggbb`, or one of black, white, red, green, blue, yellow, cyan,
        /// magenta, orange, purple, gray.
        #[arg(value_parser = parse_color)]
        color: Rgb<u8>,
        /// Key index (0 is top left, numbered row by row). Defaults to all
        /// keys.
        #[arg(short, long)]
        key: Option<u8>,
    },
    /// Show an image file (PNG, JPEG, GIF, BMP) on keys.
    Image {
        /// Image file. It's scaled to fit the key.
        file: PathBuf,
        /// Key index. Defaults to all keys.
        #[arg(short, long)]
        key: Option<u8>,
    },
    /// Show a pattern for checking image orientation and key order.
    ///
    /// Every key gets a white square in its top-left corner and a white bar
    /// along its top edge. Key colors run through the rainbow in key order,
    /// starting with red at the top left.
    TestPattern,
    /// Render a key the way the app will (image plus title) and show it.
    ///
    /// Animated GIFs play until Ctrl+C. With `--save`, writes a PNG instead
    /// and needs no device.
    Render(RenderArgs),
    /// Run the device from its profiles, as the app will, until Ctrl+C.
    ///
    /// Uses `devices.json` and the profiles in the configuration directory;
    /// a new device gets an empty Default profile. Runs the installed
    /// plugins on Node.js; their logs are in
    /// `$XDG_STATE_HOME/superdeck/logs/plugins/`.
    Run(run::RunArgs),
    /// Turn every key black.
    Clear,
    /// Reset the device to its boot logo.
    Reset,
}

#[derive(Args)]
struct RenderArgs {
    /// Key index. Defaults to all keys.
    #[arg(short, long)]
    key: Option<u8>,
    /// Image: a file (PNG, JPEG, BMP, GIF, SVG) or a `data:` URL.
    #[arg(short, long)]
    image: Option<String>,
    /// Title text; `\n` starts a new line.
    #[arg(short, long, default_value = "")]
    title: String,
    /// Vertical position of the title.
    #[arg(long, value_enum, default_value_t = Align::Middle)]
    align: Align,
    /// Font size in pixels on a 72-pixel key.
    #[arg(long, default_value_t = 12.0)]
    font_size: f32,
    /// Font family. Defaults to the bundled Noto Sans.
    #[arg(long)]
    font: Option<String>,
    /// Bold title.
    #[arg(long)]
    bold: bool,
    /// Italic title.
    #[arg(long)]
    italic: bool,
    /// Underlined title.
    #[arg(long)]
    underline: bool,
    /// Title color, as for `fill`.
    #[arg(long, value_parser = parse_color, default_value = "white")]
    color: Rgb<u8>,
    /// Write the key to a PNG file instead of a device (an animation's first
    /// frame).
    #[arg(long, value_name = "FILE")]
    save: Option<PathBuf>,
    /// Key size in pixels for `--save`.
    #[arg(long, default_value_t = 72, requires = "save", value_parser = clap::value_parser!(u32).range(1..=1024))]
    size: u32,
}

#[derive(Clone, Copy, ValueEnum)]
enum Align {
    Top,
    Middle,
    Bottom,
}

impl RenderArgs {
    fn render(&self, size: u32) -> Result<Rendered> {
        let mut renderer = Renderer::new();
        let image = match &self.image {
            None => None,
            Some(url) if url.starts_with("data:") => Some(renderer.load_data_url(url)?),
            Some(path) => {
                let data = std::fs::read(path).with_context(|| format!("could not read {path}"))?;
                Some(
                    renderer
                        .load_image(&data)
                        .with_context(|| format!("could not load {path}"))?,
                )
            }
        };
        let style = TitleStyle {
            visible: true,
            font_family: self.font.clone(),
            font_size: self.font_size,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            alignment: match self.align {
                Align::Top => TitleAlignment::Top,
                Align::Middle => TitleAlignment::Middle,
                Align::Bottom => TitleAlignment::Bottom,
            },
            color: self.color,
        };
        let title = self.title.replace("\\n", "\n");
        Ok(renderer.render(image.as_ref(), &title, &style, size))
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let mut manager = DeviceManager::new().context("could not start device discovery")?;
    match cli.command {
        Command::List => list(&mut manager),
        Command::Watch => watch(&mut manager),
        Command::Render(args @ RenderArgs { save: Some(_), .. }) => save_render(&args),
        command => {
            let (info, mut device) = open(&mut manager, cli.device.as_deref(), cli.record)?;
            run_on_device(&info, &mut device, command)
        }
    }
}

fn list(manager: &mut DeviceManager) -> Result<()> {
    let devices = manager.devices()?;
    if devices.is_empty() {
        println!("No supported Stream Deck found.");
        return Ok(());
    }
    for info in devices {
        println!(
            "{:<14} {:<34} {}",
            info.path_lossy(),
            info.model.to_string(),
            info.serial_number.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn watch(manager: &mut DeviceManager) -> Result<()> {
    for info in manager.devices()? {
        println!("present       {info} at {}", info.path_lossy());
    }
    if !manager.has_hotplug_events() {
        eprintln!("note: udev events unavailable, rescanning every second");
    }
    loop {
        for change in manager.wait_for_changes(None)? {
            match change {
                DeviceChange::Connected(info) => {
                    println!("connected     {info} at {}", info.path_lossy());
                }
                DeviceChange::Disconnected(info) => {
                    println!("disconnected  {info} at {}", info.path_lossy());
                }
            }
        }
    }
}

fn save_render(args: &RenderArgs) -> Result<()> {
    let path = args.save.as_ref().expect("called with --save");
    args.render(args.size)?
        .first_frame()
        .save(path)
        .with_context(|| format!("could not write {}", path.display()))
}

fn open(
    manager: &mut DeviceManager,
    selector: Option<&str>,
    record: Option<PathBuf>,
) -> Result<(DeviceInfo, Device)> {
    let devices = manager.devices()?;
    let info = if let Some(selector) = selector {
        devices
            .iter()
            .find(|info| {
                info.serial_number.as_deref() == Some(selector) || info.path_lossy() == selector
            })
            .with_context(|| format!("no supported device matches {selector:?}"))?
    } else {
        let first = devices.first().context("no supported Stream Deck found")?;
        if devices.len() > 1 {
            eprintln!("note: using {first}; pick another with --device");
        }
        first
    }
    .clone();

    let device = match record {
        None => manager.open(&info)?,
        Some(path) => {
            let file = File::create(&path)
                .with_context(|| format!("could not create {}", path.display()))?;
            manager.open_with(&info, |transport| {
                let recorder =
                    Recorder::new(transport, info.model, Box::new(BufWriter::new(file)))?;
                Ok(Box::new(recorder))
            })?
        }
    };
    Ok((info, device))
}

fn run_on_device(info: &DeviceInfo, device: &mut Device, command: Command) -> Result<()> {
    let spec = info.model.spec();
    match command {
        Command::List | Command::Watch => unreachable!("handled without opening a device"),
        Command::Info => {
            println!("Model:     {} ({})", info.model, info.model.id());
            println!("Path:      {}", info.path_lossy());
            println!("Serial:    {}", device.serial_number()?);
            println!("Firmware:  {}", device.firmware_version()?);
            println!(
                "Keys:      {} ({} × {})",
                spec.key_count(),
                spec.columns,
                spec.rows
            );
            println!(
                "Key image: {0} × {0} px, {1:?}",
                spec.key_size, spec.image_format
            );
        }
        Command::Events => {
            eprintln!("Press keys on the {}; Ctrl+C to stop.", info.model);
            loop {
                for event in device.read_key_events(None)? {
                    match event {
                        KeyEvent::Down(key) => println!("key {key:>2} down"),
                        KeyEvent::Up(key) => println!("key {key:>2} up"),
                    }
                }
            }
        }
        Command::Brightness { percent } => device.set_brightness(percent)?,
        Command::Fill { color, key } => {
            for key in keys(device, key)? {
                device.set_key_color(key, color)?;
            }
        }
        Command::Image { file, key } => {
            let image =
                image::open(&file).with_context(|| format!("could not read {}", file.display()))?;
            for key in keys(device, key)? {
                device.set_key_image(key, &image)?;
            }
        }
        Command::TestPattern => {
            for key in 0..device.key_count() {
                let image = test_pattern(spec.key_size, key, device.key_count());
                device.set_key_image(key, &DynamicImage::ImageRgb8(image))?;
            }
        }
        Command::Run(args) => run::run(info, device, &args)?,
        Command::Render(args) => {
            let keys = keys(device, args.key)?;
            let encode =
                |image: &RgbImage| encode_key_image(spec, &DynamicImage::ImageRgb8(image.clone()));
            match args.render(spec.key_size)? {
                Rendered::Still(image) => {
                    let data = encode(&image)?;
                    for &key in &keys {
                        device.set_key_image_data(key, &data)?;
                    }
                }
                Rendered::Animated(frames) => {
                    let frames = frames
                        .iter()
                        .map(|frame| Ok((encode(&frame.image)?, frame.delay)))
                        .collect::<Result<Vec<_>>>()?;
                    eprintln!("Playing {} frames; Ctrl+C to stop.", frames.len());
                    loop {
                        for (data, delay) in &frames {
                            for &key in &keys {
                                device.set_key_image_data(key, data)?;
                            }
                            std::thread::sleep(*delay);
                        }
                    }
                }
            }
        }
        Command::Clear => device.clear()?,
        Command::Reset => device.reset()?,
    }
    Ok(())
}

/// The keys a command applies to: `key`, or all of them.
fn keys(device: &Device, key: Option<u8>) -> Result<Vec<u8>> {
    match key {
        Some(key) if key >= device.key_count() => bail!(
            "key {key} doesn't exist; this device has keys 0 to {}",
            device.key_count() - 1
        ),
        Some(key) => Ok(vec![key]),
        None => Ok((0..device.key_count()).collect()),
    }
}

fn test_pattern(size: u32, key: u8, key_count: u8) -> RgbImage {
    const WHITE: Rgb<u8> = Rgb([255, 255, 255]);
    let background = rainbow(f32::from(key) / f32::from(key_count));
    let corner = size / 3;
    let bar = (size / 12).max(1);
    RgbImage::from_fn(size, size, |x, y| {
        if (x < corner && y < corner) || y < bar {
            WHITE
        } else {
            background
        }
    })
}

/// A fully saturated color; 0 is red, going through yellow, green, cyan,
/// blue, and magenta as `position` approaches 1.
fn rainbow(position: f32) -> Rgb<u8> {
    let hue = position.fract() * 6.0;
    let x = 1.0 - (hue % 2.0 - 1.0).abs();
    let (r, g, b) = match hue {
        h if h < 1.0 => (1.0, x, 0.0),
        h if h < 2.0 => (x, 1.0, 0.0),
        h if h < 3.0 => (0.0, 1.0, x),
        h if h < 4.0 => (0.0, x, 1.0),
        h if h < 5.0 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let channel = |c: f32| (c * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb([channel(r), channel(g), channel(b)])
}

fn parse_color(text: &str) -> Result<Rgb<u8>, String> {
    let named = match text.to_ascii_lowercase().as_str() {
        "black" => Some([0, 0, 0]),
        "white" => Some([255, 255, 255]),
        "red" => Some([255, 0, 0]),
        "green" => Some([0, 255, 0]),
        "blue" => Some([0, 0, 255]),
        "yellow" => Some([255, 255, 0]),
        "cyan" => Some([0, 255, 255]),
        "magenta" => Some([255, 0, 255]),
        "orange" => Some([255, 128, 0]),
        "purple" => Some([128, 0, 255]),
        "gray" | "grey" => Some([128, 128, 128]),
        _ => None,
    };
    if let Some(rgb) = named {
        return Ok(Rgb(rgb));
    }
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.is_ascii() {
        return Err(format!("expected #rrggbb or a color name, got {text:?}"));
    }
    let channel = |i: usize| {
        u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|_| format!("expected #rrggbb or a color name, got {text:?}"))
    };
    Ok(Rgb([channel(0)?, channel(2)?, channel(4)?]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#ff8000"), Ok(Rgb([255, 128, 0])));
        assert_eq!(parse_color("00ff00"), Ok(Rgb([0, 255, 0])));
        assert_eq!(parse_color("Red"), Ok(Rgb([255, 0, 0])));
        assert!(parse_color("#ff80").is_err());
        assert!(parse_color("#gg0000").is_err());
        assert!(parse_color("#ééé").is_err());
    }

    #[test]
    fn rainbow_starts_red_and_cycles() {
        assert_eq!(rainbow(0.0), Rgb([255, 0, 0]));
        assert_eq!(rainbow(1.0 / 3.0), Rgb([0, 255, 0]));
        assert_eq!(rainbow(2.0 / 3.0), Rgb([0, 0, 255]));
    }

    #[test]
    fn test_pattern_marks_the_top_left() {
        let image = test_pattern(72, 0, 15);
        assert_eq!(*image.get_pixel(5, 20), Rgb([255, 255, 255]));
        assert_eq!(*image.get_pixel(5, 30), Rgb([255, 0, 0]));
        assert_eq!(*image.get_pixel(60, 60), Rgb([255, 0, 0]));
        assert_eq!(*image.get_pixel(60, 30), Rgb([255, 0, 0]));
        assert_eq!(*image.get_pixel(60, 2), Rgb([255, 255, 255]));
    }

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
