// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs profiles on a [`Deck`] with made-up key presses and times.

use std::time::{Duration, Instant};

use serde_json::{Map, json};
use superdeck_core::deck::{FaceImage, Icon, PluginEvent, PluginEventKind};
use superdeck_core::model::{BrightnessMode, FontSize, TitleAlignment};
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
    ] {
        assert!(icon.svg().starts_with(b"<svg"), "{icon:?}");
    }
}
