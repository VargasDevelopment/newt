// SPDX-License-Identifier: MIT
//! Shared Pocket Pause UI and owner from pinned pause_render.cpp/pause_app.h.
use crate::{Definition, NO_SELECTION, State as TinyState, draw_note, render_unavailable};
use newt_core::{
    app::{self, Action, AppState},
    durable::Backend,
    pause as model,
    pause_store::{Namespace, Store, StoreError},
};
use newt_system_ui::{
    SYSTEM_CONTENT_LEFT as LEFT, SYSTEM_CONTENT_WIDTH as WIDTH, SYSTEM_ROW_PITCH, SystemBar,
    SystemButtons, SystemPage,
};
use newt_ui_canvas::Frame;

pub const BUILTIN_BYTES: &[u8; crate::WIRE_BYTES] = include_bytes!("pocket-pause.app");
const BODY_TOP: i32 = 150;
const SETTINGS_TOP: i32 = 196;

/// Exact zero-padded fields, including every card, identify the built-in app.
pub fn is_pocket_pause(definition: &Definition) -> bool {
    if u32::from(definition.card_count) != model::PROMPT_COUNT
        || definition.id.as_slice() != &BUILTIN_BYTES[8..41]
        || definition.version.as_slice() != &BUILTIN_BYTES[41..58]
        || definition.title.as_slice() != &BUILTIN_BYTES[58..91]
        || definition.subtitle.as_slice() != &BUILTIN_BYTES[91..156]
        || definition.note.as_slice() != &BUILTIN_BYTES[156..317]
    {
        return false;
    }
    for (index, card) in definition.cards.iter().enumerate() {
        let at = 318 + index * 99;
        if card.title.as_slice() != &BUILTIN_BYTES[at..at + 33]
            || card.detail.as_slice() != &BUILTIN_BYTES[at + 33..at + 98]
            || card.action as u8 != BUILTIN_BYTES[at + 98]
        {
            return false;
        }
    }
    true
}

pub fn render(
    definition: &Definition,
    state: &model::State,
    writable: bool,
    bar: &SystemBar,
    frame: Option<&mut Frame>,
) {
    let Some(frame) = frame else {
        return;
    };
    if !is_pocket_pause(definition) {
        render_unavailable(
            Some(b"These pause prompts could not be loaded."),
            bar,
            Some(frame),
        );
        return;
    }
    if state.view == model::View::Settings {
        let on_save = state.settings_focus == model::PROMPT_COUNT;
        let mut page = SystemPage::new(
            Some(frame),
            bar,
            SystemButtons {
                back: Some(b"Cancel"),
                confirm: Some(if on_save { b"Save" } else { b"Switch" }),
                previous: Some(b"Up"),
                next: Some(b"Down"),
            },
            Some(b"Your pauses"),
        );
        page.canvas().paragraph(
            LEFT,
            138,
            Some(b"Choose the pauses you want to see."),
            24,
            WIDTH,
            190,
        );
        for index in 0..model::PROMPT_COUNT {
            // Matches the pinned 48-byte snprintf scratch; the exact built-in
            // title is at most 32 bytes and every suffix fits before its NUL.
            let mut label = [0; 48];
            let title = &definition.cards[index as usize].title;
            let n = title.iter().position(|&b| b == 0).unwrap_or(title.len());
            label[..n].copy_from_slice(&title[..n]);
            label[n..n + 2].copy_from_slice(b": ");
            let suffix: &[u8] = if state.draft_mask & (1 << index) != 0 {
                b"On"
            } else {
                b"Off"
            };
            label[n + 2..n + 2 + suffix.len()].copy_from_slice(suffix);
            page.row(
                SETTINGS_TOP,
                index,
                Some(&label),
                state.settings_focus == index,
            );
        }
        page.row(
            SETTINGS_TOP,
            model::PROMPT_COUNT,
            Some(if writable {
                b"Save pauses"
            } else {
                b"Save unavailable"
            }),
            on_save,
        );
        let note_top = SETTINGS_TOP + (model::PROMPT_COUNT as i32 + 1) * SYSTEM_ROW_PITCH + 26;
        if !writable {
            page.canvas().paragraph(
                LEFT,
                note_top,
                Some(b"Saved pauses are unavailable. Changes cannot be saved."),
                24,
                WIDTH,
                700,
            );
        } else if state.save_unconfirmed {
            page.canvas().paragraph(
                LEFT,
                note_top,
                Some(b"Could not confirm the save. Existing pauses kept; after a restart they may change."),
                24,
                WIDTH,
                700,
            );
        }
        return;
    }
    let other = (writable && state.can_pick_other()).then_some(b"Other".as_slice());
    let failed = {
        let mut page = SystemPage::new(
            Some(&mut *frame),
            bar,
            SystemButtons {
                back: Some(b"Back"),
                confirm: Some(b"Edit"),
                previous: other,
                next: other,
            },
            Some(&definition.title),
        );
        let selected = u32::from(state.selected_prompt.0);
        if writable
            && selected < model::PROMPT_COUNT
            && state.enabled_mask & state.selected_prompt.bit() != 0
        {
            draw_note(page.canvas(), definition, selected, BODY_TOP) < 0
        } else {
            if !writable {
                page.canvas().text(
                    LEFT,
                    BODY_TOP,
                    Some(b"Saved pauses unavailable"),
                    32,
                    true,
                    WIDTH,
                    true,
                );
                page.canvas().paragraph(LEFT, BODY_TOP+70,
                    Some(b"Your saved choices could not be loaded. No pause will be shown until they are available."),
                    32, WIDTH, 700);
            } else {
                page.canvas().text(
                    LEFT,
                    BODY_TOP,
                    Some(b"A pause when you want it."),
                    32,
                    true,
                    WIDTH,
                    true,
                );
                let text: &[u8] = if state.enabled_mask != 0 {
                    b"Your saved pause is no longer enabled. Press Other to choose a pause."
                } else {
                    b"Your pauses are turned off. Press Edit to turn one on."
                };
                page.canvas()
                    .paragraph(LEFT, BODY_TOP + 70, Some(text), 32, WIDTH, 700);
            }
            false
        }
    };
    if failed {
        render_unavailable(
            Some(b"This app's text does not fit at a readable size."),
            bar,
            Some(frame),
        );
    }
}

/// State lives with the application; A/B scratch lives only during a store
/// operation. Firmware supplies its existing NvsBlobBackend, avoiding a
/// self-reference between an owned backend and its store.
#[derive(Default)]
pub struct PocketPause {
    preferences: model::Preferences,
    state: model::State,
    writable: bool,
}
impl PocketPause {
    pub fn state(&self) -> &model::State {
        &self.state
    }
    pub fn preferences(&self) -> model::Preferences {
        self.preferences
    }
    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn load<B: Backend>(&mut self, backend: &mut B) {
        let mut namespace = Namespace::new(backend);
        let loaded = Store::new(&mut namespace).load();
        self.writable = match loaded {
            Ok(value) => {
                self.preferences = value;
                true
            }
            Err(StoreError::Empty) => true,
            Err(_) => false,
        };
        self.state.enabled_mask = if self.writable {
            self.preferences.enabled_mask
        } else {
            0
        };
        self.state.draft_mask = self.state.enabled_mask;
    }

    pub fn open(&mut self, tiny: &mut TinyState, random: u32) {
        self.state.open(
            if self.writable {
                self.preferences.enabled_mask
            } else {
                0
            },
            random,
        );
        self.sync(tiny);
    }

    pub fn resume(&mut self, tiny: &TinyState) {
        let saved = if tiny.selected < model::PROMPT_COUNT {
            model::Prompt(tiny.selected as u8)
        } else {
            model::Prompt::NONE
        };
        self.state.resume(
            if self.writable {
                self.preferences.enabled_mask
            } else {
                0
            },
            saved,
            tiny.note_open,
        );
    }

    pub fn dispatch<B: Backend>(
        &mut self,
        backend: &mut B,
        app: &mut AppState,
        tiny: &mut TinyState,
        action: Action,
        random: u32,
    ) -> bool {
        let before = self.state;
        let intent = self.state.dispatch(action, random);
        if intent == model::Intent::Save {
            let mut proposed = model::Preferences {
                enabled_mask: self.state.draft_mask,
                ..self.preferences
            };
            let confirmed = self.writable && {
                let mut namespace = Namespace::new(backend);
                Store::new(&mut namespace).save(&mut proposed).is_ok()
            };
            if confirmed {
                self.preferences = proposed;
            }
            self.state.finish_save(confirmed, random);
        }
        self.sync(tiny);
        if intent == model::Intent::Sleep {
            return app::dispatch(app, Action::Sleep);
        }
        if intent == model::Intent::Home {
            return app::dispatch(app, Action::Back);
        }
        before.view != self.state.view
            || before.selected_prompt != self.state.selected_prompt
            || before.settings_focus != self.state.settings_focus
            || before.draft_mask != self.state.draft_mask
            || before.save_unconfirmed != self.state.save_unconfirmed
            || before.enabled_mask != self.state.enabled_mask
    }

    pub fn render(&self, definition: &Definition, bar: &SystemBar, frame: Option<&mut Frame>) {
        render(definition, &self.state, self.writable, bar, frame);
    }

    fn sync(&self, tiny: &mut TinyState) {
        tiny.selected = if self.state.selected_prompt == model::Prompt::NONE {
            NO_SELECTION
        } else {
            u32::from(self.state.selected_prompt.0)
        };
        tiny.note_open = self.state.view == model::View::Experience;
    }
}
