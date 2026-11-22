//! Borrowed SemVer spans, preserving frozen split/identifier comparison order.
use core::{cmp::Ordering, ffi::CStr};

struct Split<'a> {
    core: [&'a [u8]; 3],
    pre: &'a [u8],
}

fn numeric(text: &[u8]) -> bool {
    !text.is_empty() && text.iter().all(u8::is_ascii_digit)
}

fn identifiers(text: &[u8], strict_numbers: bool) -> bool {
    if text.is_empty() {
        return false;
    }
    for part in text.split(|&c| c == b'.') {
        if part.iter().any(|&c| !(c.is_ascii_alphanumeric() || c == b'-')) {
            return false;
        }
        if part.is_empty() || (strict_numbers && numeric(part) && part.len() > 1 && part[0] == b'0') {
            return false;
        }
    }
    true
}

fn split(version: &[u8]) -> Option<Split<'_>> {
    let stop = if let Some(plus) = version.iter().position(|&c| c == b'+') {
        if !identifiers(&version[plus + 1..], false) {
            return None;
        }
        &version[..plus]
    } else {
        version
    };
    let (core, pre) = if let Some(dash) = stop.iter().position(|&c| c == b'-') {
        let pre = &stop[dash + 1..];
        if !identifiers(pre, true) {
            return None;
        }
        (&stop[..dash], pre)
    } else {
        (stop, &stop[stop.len()..])
    };
    let mut parts = [&core[..0]; 3];
    let mut at = core;
    for (i, part) in parts.iter_mut().enumerate() {
        let end = if i < 2 { at.iter().position(|&c| c == b'.')? } else { at.len() };
        *part = &at[..end];
        if !numeric(part) || (part.len() > 1 && part[0] == b'0') {
            return None;
        }
        if i < 2 { at = &at[end + 1..]; }
    }
    Some(Split { core: parts, pre })
}

pub(super) fn valid_bytes(version: &[u8]) -> bool {
    split(version).is_some()
}

/// None is the frozen null C-string boundary. No UTF-8 interpretation is used.
pub fn semver2_valid(version: Option<&CStr>) -> bool {
    version.is_some_and(|v| valid_bytes(v.to_bytes()))
}

fn compare_numbers(a: &[u8], b: &[u8]) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn compare_identifiers(a: &[u8], b: &[u8]) -> Ordering {
    match (numeric(a), numeric(b)) {
        (true, true) => compare_numbers(a, b),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

fn sign(order: Ordering) -> i32 {
    match order { Ordering::Less => -1, Ordering::Equal => 0, Ordering::Greater => 1 }
}

/// Invalid or null input compares equal, as in the original public boundary.
/// Arbitrarily long numeric spans compare by length/digits, never integer parse.
pub fn semver2_compare(a: Option<&CStr>, b: Option<&CStr>) -> i32 {
    let Some(a) = a.and_then(|s| split(s.to_bytes())) else { return 0 };
    let Some(b) = b.and_then(|s| split(s.to_bytes())) else { return 0 };
    for (left, right) in a.core.iter().zip(b.core.iter()) {
        let order = compare_numbers(left, right);
        if order != Ordering::Equal {
            return sign(order);
        }
    }
    if a.pre.is_empty() || b.pre.is_empty() {
        return match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => -1,
            (false, false) => 0,
        };
    }
    let mut left = a.pre.split(|&c| c == b'.');
    let mut right = b.pre.split(|&c| c == b'.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return 0,
            (None, Some(_)) => return -1,
            (Some(_), None) => return 1,
            (Some(a), Some(b)) => {
                let order = compare_identifiers(a, b);
                if order != Ordering::Equal { return sign(order); }
            }
        }
    }
}
