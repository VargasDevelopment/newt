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
