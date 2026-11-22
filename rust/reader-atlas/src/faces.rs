// Frozen immutable face tables: only this module can construct the descriptors.
use super::*;

// The private zero-size field prevents callers from rebuilding a descriptor.
pub(super) struct Face {
    _sealed: (),
    pub(super) glyphs: &'static [AtlasGlyph; ASCII], pub(super) extra: &'static [AtlasGlyph; 246],
    pub(super) bitmap: &'static [u8], pub(super) extra_bitmap: &'static [u8],
    pub(super) classes: &'static [AtlasKernClasses; LEGACY],
    pub(super) rows: &'static [u16], pub(super) cols: &'static [u8], pub(super) values: &'static [i8],
    pub(super) ascii: &'static [AtlasKern],
}
pub(super) struct AddedFace {
    _sealed: (),
    pub(super) glyphs: &'static [AtlasGlyph; COUNT - LEGACY],
    pub(super) offsets: &'static [u32; COUNT - LEGACY],
    pub(super) classes: &'static [AddedKernClasses; COUNT],
    pub(super) rows: &'static [u16], pub(super) cols: &'static [u16], pub(super) values: &'static [i8],
}

static FACES: [Face; 4] = [
    Face { _sealed: (), glyphs: &kReaderGlyphs, extra: &kReaderExtraGlyphs, bitmap: &kReaderBitmap, extra_bitmap: &kReaderExtraBitmap, classes: &kReaderKernClasses, rows: &kReaderKernRows, cols: &kReaderKernCols, values: &kReaderKernValues, ascii: &kReaderKerning },
    Face { _sealed: (), glyphs: &kReaderBoldGlyphs, extra: &kReaderBoldExtraGlyphs, bitmap: &kReaderBoldBitmap, extra_bitmap: &kReaderBoldExtraBitmap, classes: &kReaderStyleKernClasses, rows: &kReaderBoldKernRows, cols: &kReaderBoldKernCols, values: &kReaderBoldKernValues, ascii: &[] },
    Face { _sealed: (), glyphs: &kReaderItalicGlyphs, extra: &kReaderItalicExtraGlyphs, bitmap: &kReaderItalicBitmap, extra_bitmap: &kReaderItalicExtraBitmap, classes: &kReaderStyleKernClasses, rows: &kReaderItalicKernRows, cols: &kReaderItalicKernCols, values: &kReaderItalicKernValues, ascii: &[] },
    Face { _sealed: (), glyphs: &kReaderBoldItalicGlyphs, extra: &kReaderBoldItalicExtraGlyphs, bitmap: &kReaderBoldItalicBitmap, extra_bitmap: &kReaderBoldItalicExtraBitmap, classes: &kReaderStyleKernClasses, rows: &kReaderBoldKernRows, cols: &kReaderBoldKernCols, values: &kReaderBoldKernValues, ascii: &[] },
];
pub(super) fn face(style: u8) -> &'static Face { &FACES[usize::from(style & 3)] }

static ADDED_FACES: [AddedFace; 4] = [
    AddedFace { _sealed: (), glyphs: &kAddedRegularGlyphs, offsets: &kAddedRegularOffsets, classes: &kAddedRegularClasses, rows: &kAddedRegularRows, cols: &kAddedRegularCols, values: &kAddedRegularValues },
    AddedFace { _sealed: (), glyphs: &kAddedBoldGlyphs, offsets: &kAddedBoldOffsets, classes: &kAddedBoldClasses, rows: &kAddedBoldRows, cols: &kAddedBoldCols, values: &kAddedBoldValues },
    AddedFace { _sealed: (), glyphs: &kAddedItalicGlyphs, offsets: &kAddedItalicOffsets, classes: &kAddedItalicClasses, rows: &kAddedItalicRows, cols: &kAddedItalicCols, values: &kAddedItalicValues },
    AddedFace { _sealed: (), glyphs: &kAddedBoldItalicGlyphs, offsets: &kAddedBoldItalicOffsets, classes: &kAddedBoldItalicClasses, rows: &kAddedBoldItalicRows, cols: &kAddedBoldItalicCols, values: &kAddedBoldItalicValues },
];
pub(super) fn added(style: u8) -> &'static AddedFace { &ADDED_FACES[usize::from(style & 3)] }
