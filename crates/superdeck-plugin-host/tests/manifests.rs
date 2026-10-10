// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the sample plugins in `tests/data/plugins/`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use superdeck_plugin_host::manifest::RelativePath;
use superdeck_plugin_host::{Entry, Error, discover};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn samples() -> PathBuf {
    manifest_dir().join("tests/data/plugins")
}

#[test]
fn sample_plugins() {
    let found = discover(&[samples()]);
    let uuids: Vec<_> = found.plugins.keys().map(String::as_str).collect();
    assert_eq!(
        uuids,
        [
            "com.example.counter",
            "com.example.legacy",
            "com.example.web"
        ]
    );

    let counter = &found.plugins["com.example.counter"];
    assert_eq!(counter.manifest.name, "Counter");
    assert_eq!(counter.manifest.version, "1.2.0.0");
    assert_eq!(counter.manifest.sdk_version, Some(2));
    assert_eq!(
        counter.entry,
        Entry::Node {
            script: samples().join("com.example.counter.sdPlugin/bin/plugin.js"),
            debug: Some("enabled".to_owned()),
        }
    );
    let increment = counter.action("com.example.counter.increment").unwrap();
    assert!(increment.on_keys());
    assert!(increment.supported_in_multi_actions);
    assert_eq!(increment.states[0].font_size, Some(16.0));
    let toggle = counter.action("com.example.counter.toggle").unwrap();
    assert!(toggle.disable_automatic_states);
    assert!(!toggle.supported_in_multi_actions);
    assert_eq!(toggle.states.len(), 2);
    assert!(
        !counter
            .action("com.example.counter.dial")
            .unwrap()
            .on_keys()
    );

    // Legacy manifests: a byte order mark, no UUID, backslashes in paths,
    // and the font size as a string.
    let legacy = &found.plugins["com.example.legacy"];
    let state = &legacy.manifest.actions[0].states[0];
    assert_eq!(state.image.as_ref().unwrap().as_str(), "imgs/key");
    assert_eq!(state.font_size, Some(10.0));
    assert!(matches!(&legacy.entry, Entry::Unsupported(why) if why.contains("CodePath")));

    let web = &found.plugins["com.example.web"];
    assert!(matches!(&web.entry, Entry::Unsupported(why) if why.contains("web pages")));

    assert_eq!(found.problems.len(), 1);
    assert!(
        matches!(&found.problems[0], Error::InvalidManifest { reason, .. } if reason.contains("doesn't match the directory name")),
        "{}",
        found.problems[0]
    );
}

#[test]
fn manifest_images() {
    let found = discover(&[samples()]);
    let counter = &found.plugins["com.example.counter"];
    let image = |path: &str| {
        counter
            .image_file(&RelativePath::new(path).unwrap())
            .map(|file| file.strip_prefix(&counter.dir).unwrap().to_owned())
    };
    // SVG first, then the larger PNG.
    assert_eq!(image("imgs/key"), Some("imgs/key.svg".into()));
    assert_eq!(image("imgs/key-on"), Some("imgs/key-on@2x.png".into()));
    assert_eq!(image("imgs/key-on.png"), Some("imgs/key-on.png".into()));
    assert_eq!(image("imgs/missing"), None);
}

#[test]
fn later_directories_replace_plugins() {
    let user = tempfile::tempdir().unwrap();
    let dir = user.path().join("com.example.web.sdPlugin");
    fs::create_dir(&dir).unwrap();
    fs::write(
        dir.join("manifest.json"),
        r#"{ "Name": "Web 2", "Version": "2.0", "Actions": [] }"#,
    )
    .unwrap();
    // A broken copy doesn't hide the working one.
    let broken = user.path().join("com.example.counter.sdPlugin");
    fs::create_dir(&broken).unwrap();
    fs::write(broken.join("manifest.json"), "{").unwrap();

    let found = discover(&[samples(), user.path().to_owned(), "/nonexistent".into()]);
    assert_eq!(found.plugins["com.example.web"].manifest.name, "Web 2");
    assert_eq!(found.plugins["com.example.web"].dir, dir);
    assert_eq!(
        found.plugins["com.example.counter"].manifest.name,
        "Counter"
    );
    assert_eq!(found.problems.len(), 2);
}

#[test]
fn bad_manifests() {
    let cases = [
        (
            r#"{ "Version": "1", "Actions": [] }"#,
            "missing field `Name`",
        ),
        (
            r#"{ "Name": " ", "Version": "1", "Actions": [] }"#,
            "Name is empty",
        ),
        (
            r#"{ "Name": "X", "Version": "1", "Actions": [{ "UUID": "x", "Name": "A", "States": [{}] }] }"#,
            "not a valid UUID",
        ),
        (
            r#"{ "Name": "X", "Version": "1", "Actions": [
                { "UUID": "com.example.x.a", "Name": "A", "States": [{}] },
                { "UUID": "com.example.x.a", "Name": "B", "States": [{}] } ] }"#,
            "listed twice",
        ),
        (
            r#"{ "Name": "X", "Version": "1", "Actions": [{ "UUID": "com.example.x.a", "Name": "A", "States": [] }] }"#,
            "has 0 states",
        ),
        (
            r#"{ "Name": "X", "Version": "1", "CodePath": "../x.js", "Actions": [] }"#,
            "inside the plugin directory",
        ),
    ];
    for (manifest, expected) in cases {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("com.example.x.sdPlugin");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("manifest.json"), manifest).unwrap();
        let found = discover(&[temp.path().to_owned()]);
        assert!(found.plugins.is_empty(), "{manifest}");
        let message = found.problems[0].to_string();
        assert!(message.contains(expected), "{manifest}: {message}");
    }
}

/// The schema accepts every sample manifest SuperDeck reads.
#[test]
fn schema_accepts_samples() {
    let schema = manifest_dir().join("../../sdk/protocol/schemas/manifest.schema.json");
    let mut schemas = boon::Schemas::new();
    let index = boon::Compiler::new()
        .compile(schema.to_str().unwrap(), &mut schemas)
        .unwrap();
    for uuid in [
        "com.example.counter",
        "com.example.legacy",
        "com.example.web",
    ] {
        let text = fs::read(samples().join(format!("{uuid}.sdPlugin/manifest.json"))).unwrap();
        let text = text.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&text);
        let manifest: Value = serde_json::from_slice(text).unwrap();
        if let Err(e) = schemas.validate(&manifest, index) {
            panic!("{uuid}: {e:#}");
        }
    }
}
