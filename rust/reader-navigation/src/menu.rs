use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuInfo {
    pub count: u32,
    pub focus: u32,
    pub first: u32,
    pub visible: u8,
    pub authored: bool,
    pub current_exact: bool,
    pub valid: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuRow<'a> {
    pub label: &'a [u8],
    pub index: u32,
    pub spine: u32,
    pub toc: u16,
    pub generic: bool,
    pub fragment: bool,
    pub current: bool,
    pub valid: bool,
}
impl Default for MenuRow<'_> {
    fn default() -> Self {
        Self {
            label: b"",
            index: 0,
            spine: 0,
            toc: 0,
            generic: true,
            fragment: false,
            current: false,
            valid: false,
        }
    }
}
fn window_first(focus: u32, count: u32) -> u32 {
    if count > 5 {
        focus.saturating_sub(3).min(count - 5)
    } else {
        0
    }
}
fn visit_rows(
    spines: u32,
    mapping: &[u32],
    first_mapped: u32,
    unordered: bool,
    current_fallback: u32,
    mut visit: impl FnMut(u32, usize, bool),
) {
    let front = first_mapped.min(spines);
    for spine in 0..front {
        visit(spine, 0, true);
    }
    let mut fallback_added = current_fallback < front;
    for (toc, &mapped) in mapping.iter().enumerate() {
        let fallback_here = current_fallback != NO_SPINE
            && mapped != NO_SPINE
            && if unordered {
                mapped == current_fallback
            } else {
                mapped >= current_fallback
            };
        if !fallback_added && fallback_here {
            visit(current_fallback, 0, true);
            fallback_added = true;
        }
        if mapped != NO_SPINE {
            visit(mapped, toc, false);
        }
    }
    if !unordered {
        if current_fallback != NO_SPINE && !fallback_added {
            visit(current_fallback, 0, true);
        }
        return;
    }
    for spine in front..spines {
        if !mapping.contains(&spine) {
            visit(spine, 0, true);
        }
    }
}
impl Navigation {
    /// Only mapping scratch is borrowed from the caller. The five visible rows
    /// retain owned labels; a source-backed optional-nav parse wraps this later.
    pub fn bind_menu(
        &mut self,
        book: &impl BookView,
        current: Section<'_>,
        requested_focus: u32,
        mapping: &mut [u32],
    ) -> bool {
        self.reset();
        self.projection = Projection::Menu(MenuState::default());
        if book.spine_count() == 0
            || book.spine_count() > NO_SPINE as usize
            || mapping.len() < book.toc_count()
        {
            return false;
        }
        let mapping = &mut mapping[..book.toc_count()];
        let mut first_mapped = NO_SPINE;
        let mut last_mapped = None;
        let mut unordered = false;
        for (toc, mapped) in mapping.iter_mut().enumerate() {
            *mapped = declared_spine(book, toc);
            if *mapped == NO_SPINE {
                continue;
            }
            if last_mapped.is_some_and(|last| *mapped < last) {
                unordered = true;
            }
            first_mapped = first_mapped.min(*mapped);
            last_mapped = Some(*mapped);
        }
        let mut total = 0u32;
        let mut matched = NO_SPINE;
        let mut fallback = NO_SPINE;
        let mut fallback_spine = 0;
        let current_fallback = if current.generic {
            current.spine
        } else {
            NO_SPINE
        };
        visit_rows(
            book.spine_count() as u32,
            mapping,
            first_mapped,
            unordered,
            current_fallback,
            |spine, toc, generic| {
                if spine <= current.spine && (fallback == NO_SPINE || spine >= fallback_spine) {
                    fallback = total;
                    fallback_spine = spine;
                }
                if if generic {
                    current.generic && current.spine == spine
                } else {
                    !current.generic && usize::from(current.toc) == toc
                } {
                    matched = total;
                }
                total = total.wrapping_add(1);
            },
        );
        if total == 0 {
            return false;
        }
        let exact = matched != NO_SPINE;
        let natural = if exact {
            matched
        } else if fallback != NO_SPINE {
            fallback
        } else {
            0
        };
        let focus = if requested_focus == FOCUS_CURRENT {
            natural
        } else {
            requested_focus.min(total - 1)
        };
        let first = window_first(focus, total);
        self.projection = Projection::Menu(MenuState {
            natural,
            focus,
            count: total,
            first,
            current_spine: current.spine,
            current_toc: current.toc,
            current_generic: current.generic,
            authored: last_mapped.is_some(),
            current_exact: exact,
            fallback: ContentsFallback::None,
        });
        let mut row = 0u32;
        visit_rows(
            book.spine_count() as u32,
            mapping,
            first_mapped,
            unordered,
            current_fallback,
            |spine, toc, generic| {
                let index = row;
                row = row.wrapping_add(1);
                if index < first || index >= first + 5 {
                    return;
                }
                let is_current = exact && index == matched;
                let mut target = Target {
                    spine,
                    ..Target::default()
                };
                if generic {
                    let mut label = [0; 48];
                    generic_label(spine, &mut label);
                    target.label = self.store(&label, false);
                    target.toc = NO_TOC;
                    target.flags = MENU_GENERIC | if is_current { MENU_CURRENT } else { 0 };
                } else {
                    let entry = book.toc_entry(toc).expect("mapped declaration");
                    target.label = self.store(entry.title, true);
                    target.toc = toc as u16;
                    target.flags = if c_bytes(entry.fragment).is_empty() {
                        0
                    } else {
                        FRAGMENT
                    } | if is_current { MENU_CURRENT } else { 0 };
                }
                self.targets[usize::from(self.count)] = target;
                self.count += 1;
            },
        );
        true
    }
    pub fn menu_info(&self) -> MenuInfo {
        let Projection::Menu(menu) = self.projection else {
            return MenuInfo::default();
        };
        if menu.count == 0 {
            return MenuInfo::default();
        }
        MenuInfo {
            count: menu.count,
            focus: menu.focus,
            first: menu.first,
            visible: self.count,
            authored: menu.authored,
            current_exact: menu.current_exact,
            valid: true,
        }
    }
    pub fn reuse_menu(&mut self, current: Section<'_>, requested_focus: u32) -> bool {
        let info = self.menu_info();
        let Projection::Menu(mut menu) = self.projection else {
            return false;
        };
        if !info.valid
            || menu.current_spine != current.spine
            || menu.current_toc != current.toc
            || menu.current_generic != current.generic
        {
            return false;
        }
        let focus = if requested_focus == FOCUS_CURRENT {
            menu.natural
        } else {
            requested_focus.min(info.count - 1)
        };
        if info.first != window_first(focus, info.count) {
            return false;
        }
        menu.focus = focus;
        self.projection = Projection::Menu(menu);
        true
    }
    pub fn menu_row(&self, index: u32) -> MenuRow<'_> {
        let Projection::Menu(menu) = self.projection else {
            return MenuRow::default();
        };
        if index < menu.first || index >= menu.first + u32::from(self.count) {
            return MenuRow::default();
        }
        let target = &self.targets[(index - menu.first) as usize];
        MenuRow {
            label: self.string(target.label),
            index,
            spine: target.spine,
            toc: target.toc,
            generic: target.flags & MENU_GENERIC != 0,
            fragment: target.flags & FRAGMENT != 0,
            current: target.flags & MENU_CURRENT != 0,
            valid: true,
        }
    }
}
