//! ZIP central-directory and local-header validation rules. MIT.

pub const EPUB_MAX_MEMBER_NAME_BYTES: usize = 511;
pub const EPUB_MAX_MEMBER_BYTES: u32 = 4 * 1024 * 1024;
pub const EPUB_MAX_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipCentralRecordRule {
    Ok,
    Encrypted,
    Unsupported,
}

/// APPNOTE bit order 0..15. These are SDK capability policies, not ZIP validation.
pub const ZIP_FLAG_RULES: [ZipCentralRecordRule; 16] = {
    use ZipCentralRecordRule::{Encrypted, Ok, Unsupported};
    [
        Encrypted,
        Ok,
        Ok,
        Ok,
        Ok,
        Unsupported,
        Encrypted,
        Ok,
        Ok,
        Ok,
        Ok,
        Ok,
        Ok,
        Encrypted,
        Ok,
        Ok,
    ]
};

pub const fn zip_flag_mask(rule: ZipCentralRecordRule) -> u16 {
    let mut mask = 0;
    let mut bit = 0;
    while bit < 16 {
        // Const-compatible equality without relying on a const trait impl.
        if ZIP_FLAG_RULES[bit] as u8 == rule as u8 {
            mask |= 1u16 << bit;
        }
        bit += 1;
    }
    mask
}

pub fn zip_member_flag_rule(flags: u16) -> ZipCentralRecordRule {
    let encrypted = zip_flag_mask(ZipCentralRecordRule::Encrypted);
    let unsupported = zip_flag_mask(ZipCentralRecordRule::Unsupported);
    if flags & encrypted != 0 {
        return ZipCentralRecordRule::Encrypted;
    }
    if flags & unsupported != 0 {
        return ZipCentralRecordRule::Unsupported;
    }
    ZipCentralRecordRule::Ok
}

pub fn zip_central_record_rule(
    flags: u16,
    method: u16,
    name_bytes: u16,
    uncompressed_bytes: u32,
) -> ZipCentralRecordRule {
    let flag_rule = zip_member_flag_rule(flags);
    if flag_rule != ZipCentralRecordRule::Ok {
        return flag_rule;
    }
    if (method != 0 && method != 8)
        || name_bytes == 0
        || usize::from(name_bytes) > EPUB_MAX_MEMBER_NAME_BYTES
        || uncompressed_bytes > EPUB_MAX_MEMBER_BYTES
    {
        return ZipCentralRecordRule::Unsupported;
    }
    ZipCentralRecordRule::Ok
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipLocalHeaderRule {
    Ok,
    Mismatch,
}

pub fn zip_local_header_rule(
    signature: u32,
    flags: u16,
    method: u16,
    name_bytes: u16,
    central_flags: u16,
    central_method: u16,
    central_name_bytes: u16,
) -> ZipLocalHeaderRule {
    if signature == 0x04034b50
        && flags == central_flags
        && method == central_method
        && name_bytes == central_name_bytes
    {
        ZipLocalHeaderRule::Ok
    } else {
        ZipLocalHeaderRule::Mismatch
    }
}
