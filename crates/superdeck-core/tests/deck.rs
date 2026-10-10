// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs profiles on a [`Deck`] with made-up key presses and times.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, json};
use superdeck_core::deck::{
    ActionDefaults, FLASH, FaceImage, Icon, PluginEvent, PluginEventKind, StateDefaults,
};
use superdeck_core::model::{BrightnessMode, FontSize, Resources, TitleAlignment};
use superdeck_core::{
    Action, ActionInstance, Appearance, CoreAction, Deck, DeckSettings, Effect, InstanceId, Layout,
    Page, PluginAction, Position, Profile, ProfileId,
};

const P00: Position = Position::new(0, 0);
const P10: Position = Position::new(1, 0);
const P20: Position = Position::new(2, 0);
const P30: Position = Position::new(3, 0);
const P40: Position = Position::new(4, 0);

fn plugin(name: &str) -> ActionInstance {
    ActionInstance::new(Action::Plugin(PluginAction {
        plugin: "com.example".to_owned(),
        action: format!("com.example.{name}"),
        settings: Map::new(),
        resources: Resources::new(),
    }))
}

fn core(action: CoreAction) -> ActionInstance {
    ActionInstance::new(Action::Core(action))
}

fn profile(pages: Vec<Vec<(Position, ActionInstance)>>) -> Profile {
    let mut profile = Profile::new(
        "Test",
        Layout {
            columns: 5,
            rows: 3,
        },
    );
    profile.pages = pages
        .into_iter()
        .map(|keys| Page {
            keys: keys.into_iter().collect(),
            ..Page::new()
        })
        .collect();
    profile
}

fn settings() -> DeckSettings {
    DeckSettings {
        brightness: 50,
        sleep_after: None,
    }
}

/// A deck started at time `t0`.
fn started(profile: Profile, settings: DeckSettings) -> (Deck, Instant) {
    let t0 = Instant::now();
    let mut deck = Deck::new(profile, settings, 0, t0);
    deck.start();
    (deck, t0)
}

fn plugin_events(effects: &[Effect]) -> Vec<(PluginEventKind, String, Option<Position>)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Plugin(PluginEvent {
                kind,
                action,
                position,
                ..
            }) => Some((
                *kind,
                action.trim_start_matches("com.example.").to_owned(),
                *position,
            )),
            _ => None,
        })
        .collect()
}

fn draws(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|effect| matches!(effect, Effect::Draw { .. }))
        .count()
}

fn icon(deck: &Deck, position: Position) -> Option<Icon> {
    match deck.face(position)?.image? {
        FaceImage::Builtin(icon) => Some(icon),
        _ => None,
    }
}

#[test]
fn start_shows_everything() {
    let mut deck = Deck::new(
        profile(vec![vec![
            (P00, plugin("a")),
            (P10, core(CoreAction::Sleep)),
        ]]),
        settings(),
        0,
        Instant::now(),
    );
    let effects = deck.start();
    assert_eq!(
        effects[0],
        Effect::Brightness {
            percent: 50,
            save: false
        }
    );
    assert_eq!(draws(&effects), 15);
    assert_eq!(
        plugin_events(&effects),
        [(PluginEventKind::WillAppear, "a".to_owned(), Some(P00))]
    );
    assert_eq!(icon(&deck, P10), Some(Icon::Sleep));
    assert_eq!(deck.face(P20), None);
}

#[test]
fn plugin_keys_get_down_and_up() {
    let (mut deck, t) = started(profile(vec![vec![(P00, plugin("a"))]]), settings());
    let down = deck.key_down(P00, t);
    assert_eq!(
        plugin_events(&down),
        [(PluginEventKind::KeyDown, "a".to_owned(), Some(P00))]
    );
    let up = deck.key_up(P00, t);
    assert_eq!(
        plugin_events(&up),
        [(PluginEventKind::KeyUp, "a".to_owned(), Some(P00))]
    );
    // Empty keys and stray releases do nothing.
    assert_eq!(deck.key_down(P30, t), []);
    assert_eq!(deck.key_up(P30, t), []);
}

#[test]
fn pages_wrap_around() {
    let nav = || {
        vec![
            (P00, core(CoreAction::PreviousPage)),
            (P40, core(CoreAction::NextPage)),
            (P20, core(CoreAction::PageIndicator)),
        ]
    };
    let mut first = nav();
    first.push((P10, plugin("first")));
    let mut second = nav();
    second.push((P10, plugin("second")));
    let (mut deck, t) = started(profile(vec![first, second, nav()]), settings());
    assert_eq!(deck.face(P20).unwrap().title, "1/3");

    let effects = deck.key_down(P40, t);
    assert_eq!(deck.page(), 1);
    assert_eq!(deck.face(P20).unwrap().title, "2/3");
    assert_eq!(
        plugin_events(&effects),
        [
            (
                PluginEventKind::WillDisappear,
                "first".to_owned(),
                Some(P10)
            ),
            (PluginEventKind::WillAppear, "second".to_owned(), Some(P10)),
        ]
    );
    assert_eq!(draws(&effects), 15);

    deck.key_down(P40, t);
    deck.key_down(P40, t);
    assert_eq!(deck.page(), 0);
    deck.key_down(P00, t);
    assert_eq!(deck.page(), 2);
}

#[test]
fn go_to_page() {
    let (mut deck, t) = started(
        profile(vec![
            vec![(P00, core(CoreAction::GoToPage { page: 1 }))],
            vec![(P00, core(CoreAction::GoToPage { page: 0 }))],
        ]),
        settings(),
    );
    deck.key_down(P00, t);
    assert_eq!(deck.page(), 1);
    deck.key_down(P00, t);
    assert_eq!(deck.page(), 0);
}

#[test]
fn key_up_reaches_the_pressed_key_after_a_page_change() {
    let (mut deck, t) = started(
        profile(vec![
            vec![(P00, plugin("held")), (P10, core(CoreAction::NextPage))],
            vec![(P00, plugin("other"))],
        ]),
        settings(),
    );
    deck.key_down(P00, t);
    deck.key_down(P10, t);
    let up = deck.key_up(P00, t);
    assert_eq!(
        plugin_events(&up),
        [(PluginEventKind::KeyUp, "held".to_owned(), Some(P00))]
    );
}

#[test]
fn folders_nest_and_have_a_back_key() {
    let inner = core(CoreAction::Folder {
        keys: [(P10, plugin("deep"))].into(),
    });
    let outer = core(CoreAction::Folder {
        keys: [(P10, plugin("inside")), (P20, inner)].into(),
    });
    let (mut deck, t) = started(
        profile(vec![vec![(P30, outer), (P00, plugin("top"))]]),
        settings(),
    );
    assert_eq!(icon(&deck, P30), Some(Icon::Folder));

    let effects = deck.key_down(P30, t);
    assert!(deck.in_folder());
    assert_eq!(icon(&deck, P00), Some(Icon::Back));
    assert_eq!(icon(&deck, P20), Some(Icon::Folder));
    assert_eq!(
        plugin_events(&effects),
        [
            (PluginEventKind::WillDisappear, "top".to_owned(), Some(P00)),
            (PluginEventKind::WillAppear, "inside".to_owned(), Some(P10)),
        ]
    );
    // Releasing the folder key sends nothing.
    assert_eq!(plugin_events(&deck.key_up(P30, t)), []);

    deck.key_down(P20, t);
    assert_eq!(
        plugin_events(&deck.key_down(P10, t)),
        [(PluginEventKind::KeyDown, "deep".to_owned(), Some(P10))]
    );
    deck.key_down(P00, t);
    assert!(deck.in_folder());
    deck.key_down(P00, t);
    assert!(!deck.in_folder());
    assert_eq!(deck.face(P00).unwrap().image, None);
    // Releasing the back key does nothing.
    assert_eq!(deck.key_up(P00, t), []);
}

#[test]
fn changing_page_closes_folders() {
    let folder = core(CoreAction::Folder {
        keys: [(P10, core(CoreAction::GoToPage { page: 1 }))].into(),
    });
    let (mut deck, t) = started(profile(vec![vec![(P30, folder)], vec![]]), settings());
    deck.key_down(P30, t);
    deck.key_down(P10, t);
    assert_eq!((deck.page(), deck.in_folder()), (1, false));
}

#[test]
fn user_appearance_beats_the_plugin() {
    let mut key = plugin("a");
    key.appearance = vec![Appearance {
        title: Some("Mine".to_owned()),
        title_alignment: Some(TitleAlignment::Top),
        font_size: FontSize::new(18.0),
        ..Appearance::default()
    }];
    let id = key.id;
    let (mut deck, _) = started(profile(vec![vec![(P00, key)]]), settings());

    let effects = deck.set_title(id, None, Some("Plugin's"));
    assert_eq!(draws(&effects), 1);
    deck.set_image(id, Some(0), Some("data:image/png;base64,AAAA"));
    let face = deck.face(P00).unwrap();
    assert_eq!(face.title, "Mine");
    assert_eq!(face.title_alignment, TitleAlignment::Top);
    assert!((face.font_size - 18.0).abs() < f64::EPSILON);
    assert_eq!(
        face.image,
        Some(FaceImage::DataUrl("data:image/png;base64,AAAA".to_owned()))
    );

    // In state 1 the user set nothing, so the plugin's title shows.
    deck.set_state(id, 1);
    assert_eq!(deck.face(P00).unwrap().title, "Plugin's");
    assert_eq!(deck.face(P00).unwrap().image, None);
    deck.set_title(id, Some(1), None);
    assert_eq!(deck.face(P00).unwrap().title, "");
}

#[test]
fn plugin_requests_for_unknown_or_hidden_keys() {
    let hidden = plugin("hidden");
    let hidden_id = hidden.id;
    let (mut deck, _) = started(profile(vec![vec![], vec![(P00, hidden)]]), settings());
    assert_eq!(deck.set_title(InstanceId::new(), None, Some("x")), []);
    // Not showing, so nothing to draw, but it's remembered.
    assert_eq!(deck.set_title(hidden_id, None, Some("x")), []);
    assert_eq!(deck.set_title(hidden_id, Some(2), Some("x")), []);
    assert_eq!(deck.set_state(hidden_id, 1), [Effect::ProfileChanged]);
    assert_eq!(deck.set_state(hidden_id, 1), []);
    assert_eq!(deck.set_state(hidden_id, 2), []);
    let settings = json!({ "volume": 3 }).as_object().unwrap().clone();
    assert_eq!(
        deck.set_settings(hidden_id, settings.clone()),
        [Effect::ProfileChanged]
    );
    assert_eq!(deck.set_settings(hidden_id, settings.clone()), []);
    let Action::Plugin(stored) = &deck.profile().instance(hidden_id).unwrap().action else {
        panic!("plugin action expected");
    };
    assert_eq!(stored.settings, settings);
}

#[test]
fn multi_actions_run_steps_with_delays() {
    let steps = vec![
        plugin("one"),
        core(CoreAction::Delay { milliseconds: 100 }),
        plugin("two"),
        core(CoreAction::Delay { milliseconds: 50 }),
        core(CoreAction::Brightness {
            mode: BrightnessMode::Set,
            value: 80,
        }),
    ];
    let (mut deck, t) = started(
        profile(vec![vec![(P00, core(CoreAction::MultiAction { steps }))]]),
        settings(),
    );
    assert_eq!(icon(&deck, P00), Some(Icon::MultiAction));
    let now = plugin_events(&deck.key_down(P00, t));
    assert_eq!(
        now,
        [
            (PluginEventKind::KeyDown, "one".to_owned(), None),
            (PluginEventKind::KeyUp, "one".to_owned(), None),
        ]
    );
    assert_eq!(deck.next_deadline(), Some(t + Duration::from_millis(100)));
    assert_eq!(deck.poll(t + Duration::from_millis(99)), []);
    let later = deck.poll(t + Duration::from_millis(120));
    assert_eq!(plugin_events(&later).len(), 2);
    assert_eq!(deck.next_deadline(), Some(t + Duration::from_millis(150)));
    assert_eq!(
        deck.poll(t + Duration::from_millis(150)),
        [Effect::Brightness {
            percent: 80,
            save: true
        }]
    );
    assert_eq!(deck.next_deadline(), None);
}

#[test]
fn multi_action_switch_alternates() {
    let switch = core(CoreAction::MultiActionSwitch {
        steps: [vec![plugin("on")], vec![plugin("off")]],
    });
    let id = switch.id;
    let (mut deck, t) = started(profile(vec![vec![(P00, switch)]]), settings());
    let first = deck.key_down(P00, t);
    assert!(first.contains(&Effect::ProfileChanged));
    assert_eq!(plugin_events(&first)[0].1, "on");
    assert_eq!(deck.profile().instance(id).unwrap().state, 1);
    let second = deck.key_down(P00, t);
    assert_eq!(plugin_events(&second)[0].1, "off");
    assert_eq!(deck.profile().instance(id).unwrap().state, 0);
}

#[test]
fn brightness_actions() {
    let brightness = |mode, value| core(CoreAction::Brightness { mode, value });
    let (mut deck, t) = started(
        profile(vec![vec![
            (P00, brightness(BrightnessMode::Increase, 30)),
            (P10, brightness(BrightnessMode::Decrease, 70)),
            (P20, brightness(BrightnessMode::Set, 40)),
        ]]),
        settings(),
    );
    let percent = |effects: Vec<Effect>| match effects[..] {
        [
            Effect::Brightness {
                percent,
                save: true,
            },
        ] => percent,
        _ => panic!("{effects:?}"),
    };
    assert_eq!(percent(deck.key_down(P00, t)), 80);
    assert_eq!(percent(deck.key_down(P00, t)), 100);
    assert_eq!(percent(deck.key_down(P10, t)), 30);
    assert_eq!(percent(deck.key_down(P10, t)), 0);
    assert_eq!(percent(deck.key_down(P20, t)), 40);
}

#[test]
fn sleep_and_wake() {
    let (mut deck, t) = started(
        profile(vec![vec![
            (P00, core(CoreAction::Sleep)),
            (P10, plugin("a")),
        ]]),
        settings(),
    );
    assert_eq!(deck.key_down(P00, t), [Effect::Sleep]);
    assert!(deck.is_sleeping());
    deck.key_up(P00, t);
    // The waking press does nothing else, and neither does its release.
    let wake = deck.key_down(P10, t);
    assert_eq!(wake[0], Effect::Wake);
    assert_eq!(
        wake[1],
        Effect::Brightness {
            percent: 50,
            save: false
        }
    );
    assert_eq!(draws(&wake), 15);
    assert_eq!(plugin_events(&wake), []);
    assert_eq!(deck.key_up(P10, t), []);
    assert_eq!(plugin_events(&deck.key_down(P10, t)).len(), 1);
}

#[test]
fn sleeps_when_idle() {
    let idle = DeckSettings {
        brightness: 50,
        sleep_after: Some(Duration::from_mins(1)),
    };
    let (mut deck, t) = started(profile(vec![vec![(P00, plugin("a"))]]), idle);
    assert_eq!(deck.next_deadline(), Some(t + Duration::from_mins(1)));
    // Activity pushes it back.
    let t1 = t + Duration::from_secs(30);
    deck.key_down(P00, t1);
    deck.key_up(P00, t1);
    assert_eq!(deck.poll(t + Duration::from_mins(1)), []);
    assert_eq!(deck.next_deadline(), Some(t1 + Duration::from_mins(1)));
    assert_eq!(deck.poll(t1 + Duration::from_mins(1)), [Effect::Sleep]);
    assert_eq!(deck.next_deadline(), None);
    // Once asleep, nothing more is due.
    assert_eq!(deck.poll(t1 + Duration::from_mins(10)), []);
}

#[test]
fn switch_profile_is_left_to_the_caller() {
    let other = ProfileId::new();
    let (mut deck, t) = started(
        profile(vec![vec![(
            P00,
            core(CoreAction::SwitchProfile {
                profile: other,
                page: 2,
            }),
        )]]),
        settings(),
    );
    assert_eq!(icon(&deck, P00), Some(Icon::SwitchProfile));
    assert_eq!(
        deck.key_down(P00, t),
        [Effect::SwitchProfile {
            profile: other,
            page: 2
        }]
    );
}

#[test]
fn stop_hides_plugin_keys() {
    let (mut deck, _) = started(profile(vec![vec![(P00, plugin("a"))]]), settings());
    assert_eq!(
        plugin_events(&deck.stop()),
        [(PluginEventKind::WillDisappear, "a".to_owned(), Some(P00))]
    );
}

#[test]
fn a_missing_start_page_falls_back_to_the_first() {
    let deck = Deck::new(profile(vec![vec![]]), settings(), 5, Instant::now());
    assert_eq!(deck.page(), 0);
}

#[test]
fn icons_are_svg() {
    for icon in [
        Icon::Back,
        Icon::Folder,
        Icon::SwitchProfile,
        Icon::NextPage,
        Icon::PreviousPage,
        Icon::GoToPage,
        Icon::MultiAction,
        Icon::MultiActionSwitch,
        Icon::Brightness,
        Icon::Sleep,
        Icon::Alert,
        Icon::Ok,
    ] {
        assert!(icon.svg().starts_with(b"<svg"), "{icon:?}");
    }
}

/// Manifest defaults for `com.example.toggle` (two states) and
/// `com.example.manual` (two states, switched by the plugin).
fn catalog() -> Arc<HashMap<(String, String), ActionDefaults>> {
    let two_states = |automatic_states| ActionDefaults {
        states: vec![
            StateDefaults {
                image: Some(FaceImage::File(PathBuf::from("/plugin/off.svg"))),
                title: Some("Off".to_owned()),
                title_alignment: Some(TitleAlignment::Middle),
                ..StateDefaults::default()
            },
            StateDefaults {
                image: Some(FaceImage::File(PathBuf::from("/plugin/on.svg"))),
                title: Some("On".to_owned()),
                ..StateDefaults::default()
            },
        ],
        automatic_states,
    };
    Arc::new(HashMap::from([
        (
            ("com.example".to_owned(), "com.example.toggle".to_owned()),
            two_states(true),
        ),
        (
            ("com.example".to_owned(), "com.example.manual".to_owned()),
            two_states(false),
        ),
    ]))
}

fn with_catalog(profile: Profile) -> (Deck, Instant) {
    let t0 = Instant::now();
    let mut deck = Deck::new(profile, settings(), 0, t0).with_catalog(catalog());
    deck.start();
    (deck, t0)
}

#[test]
fn manifest_defaults_show_until_the_plugin_or_user_changes_them() {
    let toggle = plugin("toggle");
    let id = toggle.id;
    let (mut deck, _) = with_catalog(profile(vec![vec![(P00, toggle)]]));
    let face = deck.face(P00).unwrap();
    assert_eq!(face.title, "Off");
    assert_eq!(face.title_alignment, TitleAlignment::Middle);
    assert_eq!(
        face.image,
        Some(FaceImage::File(PathBuf::from("/plugin/off.svg")))
    );

    deck.set_title(id, Some(0), Some("12"));
    let face = deck.face(P00).unwrap();
    assert_eq!(face.title, "12");
    // The plugin's title doesn't change the manifest's style or image.
    assert_eq!(face.title_alignment, TitleAlignment::Middle);
    assert!(matches!(face.image, Some(FaceImage::File(_))));

    deck.set_image(id, None, Some("data:image/png;base64,AA"));
    assert!(matches!(
        deck.face(P00).unwrap().image,
        Some(FaceImage::DataUrl(_))
    ));
    // Resetting gives the manifest's image back.
    deck.set_image(id, None, None);
    assert!(matches!(
        deck.face(P00).unwrap().image,
        Some(FaceImage::File(_))
    ));
}

#[test]
fn two_state_actions_switch_on_release() {
    let (toggle, manual) = (plugin("toggle"), plugin("manual"));
    let (mut deck, t) = with_catalog(profile(vec![vec![(P00, toggle), (P10, manual)]]));

    let down = deck.key_down(P00, t);
    let Effect::Plugin(event) = &down[0] else {
        panic!("{down:?}");
    };
    assert_eq!((event.state, event.states), (0, 2));
    let up = deck.key_up(P00, t);
    // keyUp has the state the key was pressed in; then it switches.
    let Effect::Plugin(event) = &up[0] else {
        panic!("{up:?}");
    };
    assert_eq!(event.state, 0);
    assert!(up.contains(&Effect::ProfileChanged));
    assert_eq!(deck.face(P00).unwrap().title, "On");

    // DisableAutomaticStates: the plugin decides.
    deck.key_down(P10, t);
    let up = deck.key_up(P10, t);
    assert_eq!(up.len(), 1);
    assert_eq!(deck.face(P10).unwrap().title, "Off");

    // Single-state actions, and those without a manifest, never switch.
    let (mut deck, t) = with_catalog(profile(vec![vec![(P00, plugin("a"))]]));
    deck.key_down(P00, t);
    assert_eq!(deck.key_up(P00, t).len(), 1);
}

#[test]
fn alerts_flash_for_a_moment() {
    let a = plugin("a");
    let id = a.id;
    let (mut deck, t) = with_catalog(profile(vec![vec![(P00, a)]]));
    let effects = deck.show_alert(id, t);
    assert_eq!(draws(&effects), 1);
    assert_eq!(icon(&deck, P00), Some(Icon::Alert));
    assert!(!deck.face(P00).unwrap().show_title);
    assert_eq!(deck.next_deadline(), Some(t + FLASH));
    assert!(deck.poll(t + FLASH / 2).is_empty());
    assert_eq!(draws(&deck.poll(t + FLASH)), 1);
    assert_eq!(icon(&deck, P00), None);
    assert_eq!(deck.next_deadline(), None);

    deck.show_ok(id, t);
    assert_eq!(icon(&deck, P00), Some(Icon::Ok));
}

#[test]
fn announcing_a_plugin_resends_its_keys() {
    let mut other = plugin("b");
    if let Action::Plugin(action) = &mut other.action {
        action.plugin = "com.other".to_owned();
    }
    let (deck, _) = with_catalog(profile(vec![vec![(P00, plugin("a")), (P10, other)]]));
    assert_eq!(
        plugin_events(&deck.announce("com.example")),
        [(PluginEventKind::WillAppear, "a".to_owned(), Some(P00))]
    );
    assert!(deck.announce("com.missing").is_empty());
}

#[test]
fn describing_instances_anywhere() {
    let a = plugin("a");
    let in_folder = plugin("f");
    let step = plugin("s");
    let (a_id, folder_id, step_id) = (a.id, in_folder.id, step.id);
    let folder = core(CoreAction::Folder {
        keys: [(P20, in_folder)].into(),
    });
    let multi = core(CoreAction::MultiAction { steps: vec![step] });
    let (mut deck, _) = with_catalog(profile(vec![
        vec![(P00, a)],
        vec![(P10, folder), (P30, multi)],
    ]));
    let kind = PluginEventKind::DidReceiveSettings;
    assert_eq!(deck.describe(kind, a_id).unwrap().position, Some(P00));
    // On another page, in a closed folder.
    assert_eq!(deck.describe(kind, folder_id).unwrap().position, Some(P20));
    assert_eq!(deck.describe(kind, step_id).unwrap().position, None);
    assert!(deck.describe(kind, InstanceId::new()).is_none());

    assert!(deck.owns("com.example", step_id));
    assert!(!deck.owns("com.other", step_id));

    let resources = Resources::from([("sound".to_owned(), "/a.wav".to_owned())]);
    assert_eq!(
        deck.set_resources(a_id, resources.clone()),
        [Effect::ProfileChanged]
    );
    assert!(deck.set_resources(a_id, resources.clone()).is_empty());
    assert_eq!(deck.describe(kind, a_id).unwrap().resources, resources);
}
