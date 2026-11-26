extern crate std;
use super::*;
#[test]
fn rejects_before_mutating() {
    let f=ReaderAtlas; let mut frame=[0xa5;52272]; let before=frame;
    let mut g=Geometry::reader(true); g.margin=-1;
    assert_eq!(f.draw(&mut frame,g,0,0,65,30,0),Err(DrawError::Geometry));
    assert_eq!(frame,before);
    for n in [0,1,52271] {
        assert_eq!(f.draw(&mut frame[..n],Geometry::reader(true),50,80,65,30,0),Err(DrawError::Frame));
        assert_eq!(frame,before);
    }
}
#[test]
fn unaligned_borrow_and_extreme_coordinates() {
    let f=ReaderAtlas; let mut guarded=[0xa5;52274]; guarded[1..52273].fill(0);
    for (x,y) in [(i32::MIN,i32::MIN),(i32::MAX,i32::MAX)] {
        assert_eq!(f.draw(&mut guarded[1..52273],Geometry::reader(true),x,y,0xffffffff,65535,255),Ok(()));
        assert!(guarded[1..52273].iter().all(|&b|b==0));
    }
    assert_eq!(f.draw(&mut guarded[1..52273],Geometry::reader(true),50,80,65,30,0),Ok(()));
    assert!(guarded[1..52273].iter().any(|&b|b!=0));
    assert_eq!((guarded[0],guarded[52273]),(0xa5,0xa5));
}
#[test]
fn explicit_metric_contract() {
    let mut f=ReaderAtlas;
    assert_eq!(f.advance(65,30,0),20);
    assert_eq!(f.kerning(65,86,30,0),-2);
    assert_eq!(f.kerning(44,86,30,0),-4);
    assert_eq!(f.advance(48,30,1),20);
    assert_eq!(f.advance(48,30,0),18);
    assert_eq!(f.advance(0x4e00,0,0),8);
    assert_eq!(f.advance(0x4e00,65535,0),90);
    assert_eq!(f.kerning(0x4e00,86,30,0),0);
    assert_eq!(f.advance(0xe0fff,30,0),0);
    assert!(!f.covers(0xe0fff));
    assert_eq!(f.advance(0x2007,30,3),16);
    assert_eq!(f.advance(0x2008,30,3),8);
    assert_eq!(f.advance(65,30,252),20);
}
#[test]
fn native_shapes_tile_their_bitmaps_exactly() {
    for native in &native::NATIVE {
        let mut end = 0;
        for face in &native.faces {
            assert_eq!(face.base as usize, end, "{} px faces must be contiguous", native.px);
            for g in &face.glyphs {
                assert_eq!(face.base as usize+usize::from(g.offset), end, "{} px shapes must be contiguous", native.px);
                end += (usize::from(g.width)+7)/8*usize::from(g.height);
            }
        }
        assert_eq!(end, native.bitmap.len());
    }
}
/// Every native glyph at every size and face draws exactly its own 1-bit
/// bitmap, decoded here rather than through `shape`: at a byte-aligned and an
/// unaligned pen (the row copy), and clipped by the margins (the per-pixel path).
#[test]
fn native_sizes_draw_their_own_bitmaps() {
    let atlas=ReaderAtlas; let geometry=Geometry::reader(false);
    let width=geometry.width as usize;
    let mut frame=std::vec![0u8; width*geometry.height as usize/8];
    let mut checked=0;
    for native in &native::NATIVE {
        let size=i32::from(native.px);
        for (style,native_face) in (0u8..).zip(&native.faces) {
            let (f,a)=(face(style),added(style));
            for (index,g) in native_face.glyphs.iter().enumerate() {
                let cp=if index < ASCII { index as u32+32 } else { u32::from(kReaderExtraCodepoints[index-ASCII]) };
                assert_eq!(glyph_index(cp), index);
                if semantic(f,a,cp) >= 0 || g.width == 0 || g.height == 0 { continue; }
                let (w,h)=(i32::from(g.width),i32::from(g.height));
                let bits=&native.bitmap[native_face.base as usize+usize::from(g.offset)..];
                let stride=(usize::from(g.width)+7)/8;
                let inked=|x: i32, y: i32| bits[y as usize*stride+x as usize/8] & (0x80>>(x%8)) != 0;
                // (left edge, top edge) of the shape: aligned, unaligned, then clipped at the left and top margins.
                for (left,top) in [(96,300),(99,300),(geometry.margin-w/2-3,geometry.margin-h/2)] {
                    let (pen,baseline)=(left-i32::from(g.left),top+i32::from(g.top));
                    let overhang=if pen >= geometry.margin && pen+cell(f,a,index,size) <= geometry.width-geometry.margin { geometry.margin/2 } else { 0 };
                    let (clip_left,clip_right)=(geometry.margin-overhang,geometry.width-geometry.margin+overhang);
                    frame.fill(0);
                    atlas.draw(&mut frame,geometry,pen,baseline,cp,native.px,style).unwrap();
                    for y in (top-2).max(0)..top+h+2 {
                        for x in (left-8).max(0)..(left+w+8).min(geometry.width) {
                            let expected=(left..left+w).contains(&x) && (top..top+h).contains(&y)
                                && (clip_left..clip_right).contains(&x) && (geometry.margin..geometry.content_bottom).contains(&y)
                                && inked(x-left,y-top);
                            let bit=y as usize*width+x as usize;
                            assert_eq!(frame[bit/8] & (0x80>>(bit%8)) != 0, expected,
                                "native glyph must draw its own bitmap: {} px style {style} U+{cp:04X} at ({x},{y}), shape at ({left},{top})", native.px);
                        }
                    }
                    checked+=1;
                }
            }
        }
    }
    assert!(checked > 4*4*300*3);
}
#[test]
fn non_latin_glyphs_resample_at_native_sizes() {
    let f=ReaderAtlas; let mut frame=[0u8;52272];
    f.draw(&mut frame,Geometry::reader(false),100,200,0x2192,22,0).unwrap();
    assert!(frame.iter().any(|&b| b != 0));
}
#[test]
fn ink_extents_matches_draw_for_every_atlas_glyph() {
    let mut f = ReaderAtlas;
    assert_eq!(f.ink_extents(0x01d9, 75, 0), Some((80, 0))); // Ǚ: top bearing 32 at 30 px -> 80 px
    assert_eq!(f.ink_extents(0x210a, 75, 0), Some((28, 30))); // ℊ: height 23, top 11 -> 58 - 28 = 30 px
    assert_eq!(f.ink_extents(b'g' as u32, 75, 0), Some((40, 18)));
    assert_eq!(f.ink_extents(b'_' as u32, 75, 0), Some((0, 13)));
    for blank in [0x20u32, 0xa0, 0xad, 0x200b, 0xfe0f, 0xfeff] {
        assert_eq!(f.ink_extents(blank, 75, 0), Some((0, 0)), "U+{blank:04X} must report zero ink");
    }
    // Every atlas glyph across all 4 styles agrees with draw() on empty vs non-empty and never overflows.
    for style in 0u8..4 {
        let (fc, ad) = (face(style), added(style));
        for index in 0..COUNT {
            let cp = if index < ASCII {
                index as u32 + 32
            } else if index < LEGACY {
                u32::from(kReaderExtraCodepoints[index - ASCII])
            } else {
                u32::from(kReaderAddedCodepoints[index - LEGACY])
            };
            for px in [22u16, 30, 75] {
                let (above, below) = f.ink_extents(cp, px, style).unwrap();
                let g = shape(fc, ad, style, index, effective_size(px));
                let draws = semantic(fc, ad, cp) < 0 && g.width > 0 && g.height > 0;
                assert_eq!(above > 0 || below > 0, draws, "U+{cp:04X} style {style} px {px}");
            }
        }
    }
    // Concrete pixel check: placing Ǚ at margin + above and ℊ at content_bottom - below loses 0 ink bits.
    let geom = Geometry::reader(false);
    let mut center = std::vec![0u8; 52272];
    let mut edge = std::vec![0u8; 52272];
    let bits = |buf: &[u8]| buf.iter().map(|b| b.count_ones()).sum::<u32>();
    for (cp, at_top) in [(0x01d9u32, true), (0x210a, false), (b'g' as u32, false)] {
        let (above, below) = f.ink_extents(cp, 75, 0).unwrap();
        center.fill(0);
        edge.fill(0);
        f.draw(&mut center, geom, 100, 400, cp, 75, 0).unwrap();
        let baseline = if at_top { geom.margin + i32::from(above) } else { geom.content_bottom - i32::from(below) };
        f.draw(&mut edge, geom, 100, baseline, cp, 75, 0).unwrap();
        assert!(bits(&center) > 0);
        assert_eq!(bits(&edge), bits(&center), "U+{cp:04X} clipped at edge baseline {baseline}");
    }
}
