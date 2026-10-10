// SPDX-License-Identifier: GPL-3.0-or-later

//! Replays the captures in `tests/captures/` against the driver.

use std::path::Path;
use std::time::Duration;

use superdeck_device::{Capture, Device, KeyEvent, Model, Replay};

/// Opens a fake device that plays back `name`.
fn replay(name: &str) -> (Device, Replay) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/captures")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap();
    let capture = Capture::parse(&text).unwrap();
    let model = capture.model.expect("capture names its model");
    let replay = Replay::new(capture.ops);
    let device = Device::with_transport(model, Box::new(replay.clone())).unwrap();
    (device, replay)
}

fn events(device: &mut Device) -> Vec<KeyEvent> {
    device.read_key_events(Some(Duration::ZERO)).unwrap()
}

/// The scenario every capture follows: identify, brightness 50 %, a
/// 1500-byte raw image on key 3, then `last_key` and key 0 pressed and
/// released, then a reset.
fn standard_session(
    name: &str,
    model: Model,
    serial: &str,
    firmware: &str,
    last_key: u8,
) -> (Device, Replay) {
    let (mut device, replay) = replay(name);
    assert_eq!(device.model(), model);
    assert_eq!(device.serial_number().unwrap(), serial);
    assert_eq!(device.firmware_version().unwrap(), firmware);
    device.set_brightness(50).unwrap();
    device.set_key_image_data(3, &[0xab; 1500]).unwrap();

    assert_eq!(events(&mut device), [KeyEvent::Down(0)]);
    assert_eq!(events(&mut device), [KeyEvent::Down(last_key)]);
    assert!(device.pressed_keys()[0] && device.pressed_keys()[usize::from(last_key)]);
    assert_eq!(events(&mut device), [KeyEvent::Up(0)]);
    assert_eq!(events(&mut device), [KeyEvent::Up(last_key)]);
    assert!(device.pressed_keys().iter().all(|&down| !down));
    (device, replay)
}

fn finish(mut device: Device, replay: &Replay) {
    device.reset().unwrap();
    replay.finish().unwrap();
}

#[test]
fn mk2() {
    let (device, replay) = standard_session("mk2.cap", Model::Mk2, "AL12K1A01234", "1.00.012", 14);
    finish(device, &replay);
}

#[test]
fn xl() {
    let (device, replay) = standard_session("xl.cap", Model::XlV2, "AL12K1A01234", "1.00.012", 31);
    finish(device, &replay);
}

#[test]
fn neo_ignores_touch_keys() {
    let (mut device, replay) =
        standard_session("neo.cap", Model::Neo, "AL12K1A01234", "1.00.012", 7);
    assert_eq!(events(&mut device), []);
    finish(device, &replay);
}

#[test]
fn original_numbers_keys_right_to_left() {
    let (device, replay) = standard_session(
        "original.cap",
        Model::Original,
        "AL12H1A01234",
        "1.0.170133",
        14,
    );
    finish(device, &replay);
}

#[test]
fn mini() {
    let (device, replay) =
        standard_session("mini.cap", Model::MiniMk2, "AL12H1A01234", "1.0.170133", 5);
    finish(device, &replay);
}

#[test]
fn replay_rejects_wrong_traffic() {
    let (mut device, _replay) = replay("mk2.cap");
    // The capture expects the serial number query first.
    let error = device.set_brightness(50).unwrap_err().to_string();
    assert!(error.contains("expected feature-get"), "{error}");
}

#[test]
fn out_of_range_keys_are_rejected() {
    let (mut device, _replay) = replay("mini.cap");
    assert!(device.set_key_image_data(6, &[0; 10]).is_err());
    assert!(device.set_key_image_data(0, &vec![0; 300_000]).is_err());
}
