use newt_device_ui::*;
#[inline(never)]
#[unsafe(no_mangle)]
pub fn paint_device(
    state: &AppState,
    info: &DeviceInfo<'_>,
    bar: &SystemBar,
    frame: &mut Frame,
    scratch: Option<&mut Frame>,
    zone: newt_core::time_zone::TimeZone,
    source: Option<&mut dyn FnMut(StatsGraphPosition, u32) -> stats::StatsScreenView>,
    build: BuildIdentity<'_>,
    choices: SleepScreenChoices,
) {
    render_device_ui(
        state,
        info,
        bar,
        Some(frame),
        zone,
        scratch,
        source,
        build,
        choices,
        Default::default(),
        Default::default(),
    );
}
#[inline(never)]
#[unsafe(no_mangle)]
pub fn paint_sleep(
    info: &DeviceInfo<'_>,
    home: &HomeData<'_>,
    extras: &SleepExtras,
    frame: &mut Frame,
    geometry: Geometry,
    selector: bool,
) {
    render_sleep_ui(info, Some(frame), geometry, selector);
    render_sleep_extras(extras, home, Some(frame), geometry, selector);
}
// Night inversion belongs to renderSleepScene's caller, after the complete paint.
#[inline(never)]
#[unsafe(no_mangle)]
pub fn paint_off(home: &HomeData<'_>, frame: &mut Frame, geometry: Geometry, selector: bool) {
    render_sleep_extras(
        &SleepExtras::default(),
        home,
        Some(frame),
        geometry,
        selector,
    );
}
