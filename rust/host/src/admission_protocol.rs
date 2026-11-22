//! Test-only host adapter. Parsing/allocation remain outside newt-core.
use super::{hex, number};
use newt_core::{utf8, zip_record_rules as zip};

fn central(rule: zip::ZipCentralRecordRule) -> &'static str {
    match rule {
        zip::ZipCentralRecordRule::Ok => "ok",
        zip::ZipCentralRecordRule::Encrypted => "encrypted",
        zip::ZipCentralRecordRule::Unsupported => "unsupported",
    }
}

pub(super) fn run(fields: &[&str]) -> Option<String> {
    Some(match fields[0] {
        "utf8_next" => {
            let bytes = hex(fields[1]);
            let present = number(fields[2]) != 0;
            let length = usize::try_from(number(fields[3])).unwrap();
            let text = if present {
                Some(&bytes[..length])
            } else {
                None
            };
            let mut offset = usize::try_from(number(fields[4])).unwrap();
            let mut cp = u32::try_from(number(fields[5])).unwrap();
            let mut result = Vec::new();
            for _ in 0..number(fields[6]) {
                let ok = utf8::next(text, &mut offset, &mut cp);
                result.push(format!("{}:{}:{}", u8::from(ok), offset, cp));
            }
            result.join(" ")
        }
        "zip_constants" => format!(
            "{} {} {} {} {} {}",
            zip::EPUB_MAX_MEMBER_NAME_BYTES,
            zip::EPUB_MAX_MEMBER_BYTES,
            zip::EPUB_MAX_EXPANDED_BYTES,
            zip::zip_flag_mask(zip::ZipCentralRecordRule::Ok),
            zip::zip_flag_mask(zip::ZipCentralRecordRule::Encrypted),
            zip::zip_flag_mask(zip::ZipCentralRecordRule::Unsupported)
        ),
        "zip_table" => zip::ZIP_FLAG_RULES
            .iter()
            .map(|&r| central(r))
            .collect::<Vec<_>>()
            .join(" "),
        "zip_flags" => central(zip::zip_member_flag_rule(
            u16::try_from(number(fields[1])).unwrap(),
        ))
        .into(),
        "zip_central" => central(zip::zip_central_record_rule(
            u16::try_from(number(fields[1])).unwrap(),
            u16::try_from(number(fields[2])).unwrap(),
            u16::try_from(number(fields[3])).unwrap(),
            u32::try_from(number(fields[4])).unwrap(),
        ))
        .into(),
        "zip_local" => match zip::zip_local_header_rule(
            u32::try_from(number(fields[1])).unwrap(),
            u16::try_from(number(fields[2])).unwrap(),
            u16::try_from(number(fields[3])).unwrap(),
            u16::try_from(number(fields[4])).unwrap(),
            u16::try_from(number(fields[5])).unwrap(),
            u16::try_from(number(fields[6])).unwrap(),
            u16::try_from(number(fields[7])).unwrap(),
        ) {
            zip::ZipLocalHeaderRule::Ok => "ok".into(),
            zip::ZipLocalHeaderRule::Mismatch => "mismatch".into(),
        },
        _ => return None,
    })
}
