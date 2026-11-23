// SPDX-License-Identifier: MIT
//! The app Apps runs. The durable record is its authority: `restore` is the
//! only constructor, so startup cannot hold an app without reading the record.
//! An install replaces the running app only through `activate`, which
//! app_store::install calls after the new record is committed and selected.
use super::package;
use newt_core::{app_store::PACKAGE_BYTES, durable::Backend};
use newt_tiny_app::{Definition, decode};

pub struct InstalledApp {
    /// Boot-owned storage, kept whatever the restore found, so a later
    /// install can fill it.
    definition: &'static mut Definition,
    unavailable: Option<&'static [u8]>,
}

impl InstalledApp {
    pub fn restore(store: &mut impl Backend, definition: &'static mut Definition) -> Self {
        let unavailable = package::load_boot(store, definition).err();
        Self { definition, unavailable }
    }

    /// None while the boot explanation stands in for the app.
    pub fn running(&self) -> Option<&Definition> {
        self.unavailable.is_none().then_some(&*self.definition)
    }

    pub fn unavailable(&self) -> Option<&'static [u8]> {
        self.unavailable
    }

    /// The selected record's package; false leaves the running app as it was.
    pub fn activate(&mut self, selected: &[u8; PACKAGE_BYTES]) -> bool {
        let decoded = decode(Some(selected), self.definition);
        if decoded {
            self.unavailable = None;
        }
        decoded
    }
}
