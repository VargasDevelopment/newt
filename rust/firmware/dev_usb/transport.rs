// SPDX-License-Identifier: MIT
//! Attach the existing exclusive Serial loan to the protocol's channel port.
use super::Channel;
use crate::serial_console::DevChannel;

impl Channel for DevChannel<'_> {
    fn connected(&mut self) -> bool { self.connected() }
    fn read_byte(&mut self) -> i32 { self.read_byte() }
    fn write(&mut self, bytes: &[u8]) -> usize { self.write(bytes) }
    fn flush_tx_tail(&mut self) -> bool { self.flush_tx_tail() }
    fn now(&mut self) -> u32 { self.now() }
}
