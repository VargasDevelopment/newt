//! Language-neutral ASCII protocol; host allocation does not enter the library.
use newt_core::page_renderer::Font;
use newt_reader_atlas::{Geometry, ReaderAtlas};
use std::io::{Read, Write};
fn main() {
    if run().is_err() { std::process::exit(2); }
}
fn run() -> Result<(),String> {
    let mut input = Vec::new();
    std::io::stdin().take(1_048_577).read_to_end(&mut input).map_err(|e|e.to_string())?;
    if input.len()>1_048_576 || input.last()!=Some(&b'\n') || input.contains(&b'\r') { return Err("framing".into()); }
    let text = std::str::from_utf8(&input).map_err(|_|"utf8")?;
    let mut font = ReaderAtlas;
    let mut output = std::io::BufWriter::new(std::io::stdout());
    let mut encoded = vec![0u8; 2 * 52273];
    for line in text[..text.len()-1].split('\n') {
        let fields: Vec<_> = line.split(' ').collect();
        if fields.len()!=13 || fields[0]!="A1" { return Err("fields".into()); }
        let mut v = [0i64;12];
        for (i,f) in fields[1..].iter().enumerate() {
            let n = f.parse::<i64>().map_err(|_|"integer")?;
            if n.to_string()!=*f { return Err("noncanonical integer".into()); } v[i]=n;
        }
        let cp=u32::try_from(v[0]).map_err(|_|"cp")?; let right=u32::try_from(v[1]).map_err(|_|"right")?;
        let px=u16::try_from(v[2]).map_err(|_|"px")?; let style=u8::try_from(v[3]).map_err(|_|"style")?;
        let cv = |i:usize| i32::try_from(v[i]).map_err(|_|"coordinate".to_owned());
        let g=Geometry { width:cv(4)?,height:cv(5)?,margin:cv(6)?,content_bottom:cv(7)?,bar_y:0,footer_y:0 };
        let x=cv(8)?; let baseline=cv(9)?;
        let len=usize::try_from(v[10]).map_err(|_|"length")?;
        let fill=u8::try_from(v[11]).map_err(|_|"fill")?;
        if len>52273 { return Err("length bound".into()); }
        let mut frame=vec![fill;len];
        let status=match font.draw(&mut frame,g,x,baseline,cp,px,style) { Ok(())=>0,Err(newt_reader_atlas::DrawError::Geometry)=>1,Err(_)=>2 };
        write!(output,"A1 {} {} {} {} {} {} ",font.advance(cp,px,style),font.kerning(cp,right,px,style),font.line_height(px),font.ascent(px),u8::from(font.covers(cp)),status).map_err(|e|e.to_string())?;
        for (&b, pair) in frame.iter().zip(encoded.chunks_exact_mut(2)) {
            pair[0] = b"0123456789abcdef"[(b >> 4) as usize];
            pair[1] = b"0123456789abcdef"[(b & 15) as usize];
        }
        output.write_all(&encoded[..frame.len() * 2]).map_err(|e|e.to_string())?;
        writeln!(output).map_err(|e|e.to_string())?;
    }
    output.flush().map_err(|e|e.to_string())
}
