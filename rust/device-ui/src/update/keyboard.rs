// SPDX-License-Identifier: MIT
//! QwertyEn, numberRow=true, langKey=false from pinned FreeInk cd6f5b83
//! FreeInkUI.cpp::buildKeyboardLayout and KeyboardNavigator. No held layout.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind { Text, Space, Shift, Mode, Delete, Submit }
#[derive(Clone, Copy)]
pub(super) struct Key { pub kind: Kind, pub byte: u8, pub width: i32 }
pub(super) fn rows(symbols: bool) -> u8 { if symbols { 4 } else { 5 } }
pub(super) fn columns(symbols: bool, shifted: bool, row: u8) -> u8 {
    if symbols {
        match row { 0 => 10, 1 => if shifted { 6 } else { 9 }, 2 => 8, 3 => 4, _ => 0 }
    } else {
        match row { 0 | 1 => 10, 2 | 3 => 9, 4 => 3, _ => 0 }
    }
}
pub(super) fn focus(symbols: bool, shifted: bool, row: u8, column: u8) -> (u8, u8) {
    let row = row.min(rows(symbols) - 1);
    (row, column.min(columns(symbols, shifted, row) - 1))
}
pub(super) fn moved(symbols: bool, shifted: bool, row: u8, column: u8, next: bool, vertical: bool) -> (u8, u8) {
    let (mut row, mut column) = focus(symbols, shifted, row, column);
    let delta = if next { 1 } else { -1 };
    if vertical {
        let old = columns(symbols, shifted, row);
        row = (i16::from(row) + delta).rem_euclid(i16::from(rows(symbols))) as u8;
        let new = columns(symbols, shifted, row);
        if old != new { column = (u16::from(column) * u16::from(new) / u16::from(old)) as u8; }
    } else {
        column = (i16::from(column) + delta).rem_euclid(i16::from(columns(symbols, shifted, row))) as u8;
    }
    focus(symbols, shifted, row, column)
}
pub(super) fn key(symbols: bool, shifted: bool, row: u8, column: u8) -> Option<Key> {
    if column >= columns(symbols, shifted, row) { return None; }
    let control = |kind, width| Some(Key { kind, byte: 0, width });
    let (bytes, at): (&[u8], usize) = if symbols {
        match row {
            0 => (if shifted { b"[]{}<>^*+=" } else { b"1234567890" }, column as usize),
            1 => (if shifted { b"_\\|~`%" } else { b"-/:;()$&@" }, column as usize),
            2 if column == 7 => return control(Kind::Delete, 3),
            2 => (b".,?!'\"#", column as usize),
            3 => return match column { 0 => control(Kind::Mode, 4), 1 => control(Kind::Shift, 4),
                2 => Some(Key { kind: Kind::Space, byte: b' ', width: 8 }), _ => control(Kind::Submit, 4) },
            _ => return None,
        }
    } else {
        match row {
            0 => (if shifted { b"!@#$%^&*()" } else { b"1234567890" }, column as usize),
            1 => (if shifted { b"QWERTYUIOP" } else { b"qwertyuiop" }, column as usize),
            2 => (if shifted { b"ASDFGHJKL" } else { b"asdfghjkl" }, column as usize),
            3 if column == 0 => return control(Kind::Shift, 3),
            3 if column == 8 => return control(Kind::Delete, 3),
            3 => (if shifted { b"ZXCVBNM" } else { b"zxcvbnm" }, column as usize - 1),
            4 => return match column { 0 => control(Kind::Mode, 4),
                1 => Some(Key { kind: Kind::Space, byte: b' ', width: 12 }), _ => control(Kind::Submit, 4) },
            _ => return None,
        }
    };
    Some(Key { kind: Kind::Text, byte: bytes[at], width: 2 })
}
