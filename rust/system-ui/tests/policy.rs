use newt_system_ui::*;
#[test]
fn status_policy_and_shell_draws() {
    let mut out = [0; 48];
    assert_eq!(
        format_system_status(&mut out, false, true, 84),
        "Offline · 84%".as_bytes()
    );
    assert_eq!(
        format_system_status(&mut out, true, true, 101),
        "Wi-Fi · --%".as_bytes()
    );
    assert_eq!(
        format_system_status(&mut out, true, true, 100),
        "Wi-Fi · 100%".as_bytes()
    );
    assert!(!system_footer_shows(None, 0));
    assert!(!system_footer_shows(None, 4));
    draw_frosted_system_base(&SystemBar::default(), HomePlane::Monochrome, None);
    draw_frosted_system_header(&SystemBar::default(), HomePlane::Lsb, None);
    draw_canonical_wordmark(None, HomePlane::Msb);
    draw_system_footer(None, SystemButtons::default(), HomePlane::Monochrome);
}
#[test]
fn list_domain_and_rail() {
    assert!(ListOverlay::new(None, &[]).is_none());
    let labels = [Some(b"single" as &[u8])];
    let list = ListOverlay::new(None, &labels).unwrap();
    let mut focus = 255;
    assert_eq!(
        dispatch_list_overlay(&list, &mut focus, Action::Read),
        ListOverlayResult::Unhandled
    );
    assert_eq!(focus, 0);
    let buttons = list_overlay_buttons(&list, focus);
    assert_eq!(buttons.back, Some(b"Cancel" as &[u8]));
    assert_eq!(buttons.confirm, Some(b"Save" as &[u8]));
    assert_eq!(buttons.previous, None);
    assert_eq!(buttons.next, None);
    render_list_overlay(&list, focus, None, None);
}

#[test]
fn clock_is_owned_and_truncated_at_construction() {
    let mut bytes = *b"12:34:56\0";
    let clock = core::ffi::CStr::from_bytes_with_nul(&bytes).unwrap();
    let bar = SystemBar::new(Some(clock), ClockFormat::TwelveHour, true, 84);
    bytes[0] = b'9';
    assert_eq!(bytes[0], b'9');
    assert_eq!(bar.clock, *b"12:34\0");
    assert_eq!(SystemBar::default().clock, *b"--:--\0");
    assert_eq!(
        SystemBar::new(Some(c""), ClockFormat::TwentyFourHour, false, 0).clock,
        [0; 6]
    );
}
