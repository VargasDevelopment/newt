use core::{fmt, ops::Deref};

/// Fixed NUL-terminated UTF-8 storage, including the terminator allowance.
#[derive(Clone, Copy)]
pub(crate) struct Text<const N: usize> {
    bytes: [u8; N],
}
impl<const N: usize> Text<N> {
    pub(crate) fn as_c_str(&self) -> Option<&core::ffi::CStr> {
        core::ffi::CStr::from_bytes_until_nul(&self.bytes).ok()
    }
    pub(crate) const fn empty() -> Self {
        Self { bytes: [0; N] }
    }
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        let mut out = Self::empty();
        fmt::Write::write_str(&mut out, value).ok()?;
        Some(out)
    }
    pub(crate) fn truncate(&mut self, len: usize) {
        assert!(len <= self.len() && self.is_char_boundary(len));
        self.bytes[len] = 0;
    }
}
impl<const N: usize> Deref for Text<N> {
    type Target = str;
    fn deref(&self) -> &str {
        let len = self.bytes.iter().position(|&b| b == 0).unwrap_or(N);
        core::str::from_utf8(&self.bytes[..len]).expect("UTF-8 copied from str")
    }
}
impl<const N: usize> fmt::Write for Text<N> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let len = self.len();
        let end = len.checked_add(value.len()).ok_or(fmt::Error)?;
        if end >= N || value.contains('\0') {
            return Err(fmt::Error);
        }
        self.bytes[len..end].copy_from_slice(value.as_bytes());
        self.bytes[end] = 0;
        Ok(())
    }
}
pub(crate) type Path = Text<256>;
