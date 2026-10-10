// SPDX-License-Identifier: AGPL-3.0-or-later
//! Independent exercise driver; agent tooling, not a product dependency.
use atelier_geom::{Point, Rect};
use atelier_model::{EditKind, History};
use atelier_model::{CodecError, Decoder, Encoder, Operation, Transaction, TransactionalState};
use atelier_render::{Color, Command, DisplayList, DisplayPath, DisplayTransform, FillRule,
    PageInfo, PathElement, PositionedGlyph, StrokeStyle, LineCap, LineJoin,
    SvgImage, SvgLimits, SvgResourceProvider, render_svg};
use atelier_xml::{Event, Name, Reader, Writer};
use std::error::Error;

struct Resources { glyph: DisplayPath, png: Vec<u8> }
impl SvgResourceProvider for Resources {
    fn glyph_outline(&self, font: u64, glyph: u32) -> Option<&DisplayPath> {
        (font == 1 && glyph == 7).then_some(&self.glyph)
    }
    fn image(&self, image: u64) -> Option<SvgImage<'_>> {
        (image == 1).then_some(SvgImage { mime_type: "image/png", bytes: &self.png })
    }
}
fn point(x: f64, y: f64) -> Point { Point::try_new(x, y).expect("fixed finite point") }
fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::from_xywh(x, y, w, h).expect("fixed rectangle")
}
fn rectangle(x: f64, y: f64, w: f64, h: f64) -> DisplayPath {
    DisplayPath::new(vec![PathElement::MoveTo(point(x,y)),
        PathElement::LineTo(point(x+w,y)), PathElement::LineTo(point(x+w,y+h)),
        PathElement::LineTo(point(x,y+h)), PathElement::Close]).unwrap()
}
fn fill(path: DisplayPath, r: f32, g: f32, b: f32, a: f32) -> Command {
    Command::FillPath { path, color: Color::new(r,g,b,a).unwrap(), rule: FillRule::NonZero }
}
fn svg(destination: &str, png_path: &str) -> Result<(), Box<dyn Error>> {
    let resources = Resources { glyph: rectangle(0.,0.,8.,10.), png: std::fs::read(png_path)? };
    let commands = vec![
        fill(rectangle(0.,0.,300.,180.),1.,1.,1.,1.),
        // Clip is fixed at x=15..35, even while the drawing matrix changes.
        Command::PushClip { bounds: rect(15.,15.,20.,20.) },
        Command::PushTransform { transform: DisplayTransform { a:1.,b:0.,c:0.,d:1.,e:10.,f:10. } },
        fill(rectangle(0.,0.,40.,40.),1.,0.,0.,1.),
        Command::PopClip,
        fill(rectangle(40.,0.,10.,10.),0.,0.,1.,1.),
        Command::PopTransform,
        // Translation followed by scale: geometry at x=80..100, y=20..40.
        Command::PushTransform { transform: DisplayTransform { a:1.,b:0.,c:0.,d:1.,e:40.,f:10. } },
        Command::PushTransform { transform: DisplayTransform { a:2.,b:0.,c:0.,d:2.,e:0.,f:0. } },
        fill(rectangle(0.,0.,10.,10.),0.,1.,0.,1.),
        Command::PopTransform, Command::PopTransform,
        fill(rectangle(110.,10.,20.,20.),1.,0.,0.,0.5),
        Command::DrawGlyphRun { font_id:1, color:Color::new(0.,0.,0.,1.)?, glyphs:vec![
            PositionedGlyph { glyph_id:7, position:point(150.,10.), advance:point(12.,0.) },
            PositionedGlyph { glyph_id:7, position:point(170.,10.), advance:point(12.,0.) }] },
        Command::DrawImage { image_id:1, bounds:rect(200.,10.,20.,20.) },
        Command::StrokePath { path: DisplayPath::new(vec![PathElement::MoveTo(point(10.,80.)),
            PathElement::QuadraticTo { control:point(30.,50.), to:point(50.,80.) },
            PathElement::CubicTo { control1:point(70.,110.), control2:point(90.,50.), to:point(110.,80.) }])?,
            color:Color::new(0.,0.,0.7,1.)?, style:StrokeStyle::new(3.,LineCap::Round,LineJoin::Round,4.)? },
        Command::Link { bounds:rect(10.,130.,80.,20.), target:"https://example.invalid/?x=\"<&🙂".into() },
        Command::Structure { role:"paragraph \"<&🙂".into(), node_id:42 },
    ];
    let list=DisplayList::new(PageInfo::new(rect(0.,0.,300.,180.))?,commands)?;
    let first=render_svg(&list,&resources,SvgLimits::default())?;
    assert_eq!(first,render_svg(&list,&resources,SvgLimits::default())?);
    std::fs::write(destination,first)?;
    println!("{{\"svg_deterministic\":true}}");
    Ok(())
}
fn opaque(input: &str, destination: &str) -> Result<(), Box<dyn Error>> {
    let bytes=std::fs::read(input)?;
    let mut reader=Reader::new(bytes.as_slice());
    let mut starts=0;
    while let Some(event)=reader.next_event()? {
        if matches!(event,Event::StartElement {..}) { starts+=1; }
        if starts==2 { break; }
    }
    assert_eq!(starts,2);
    let tree=reader.capture_opaque()?;
    let mut writer=Writer::new(Vec::new())?;
    let name=Name { prefix:Some("ns0".into()), local:"target".into(), namespace:Some("urn:hostile".into()) };
    writer.write_event(&Event::StartElement { name:name.clone(), attributes:vec![], empty:false })?;
    writer.write_opaque(&tree)?;
    writer.write_event(&Event::EndElement { name })?;
    std::fs::write(destination,writer.finish()?)?;
    // A captured empty start must leave the following sibling untouched.
    let remaining=reader.next_event()?;
    println!("{{\"captured\":true,\"following_event\":{}}}",remaining.is_some());
    Ok(())
}

struct Number(i64);
impl TransactionalState for Number {
    type Checkpoint=i64;
    fn checkpoint(&self)->i64 { self.0 }
    fn restore(&mut self, value:i64) { self.0=value; }
}
struct Set(i64);
impl Operation<Number> for Set {
    type Error=&'static str;
    fn apply(&self, n:&mut Number)->Result<(),Self::Error> { n.0=self.0; Ok(()) }
    fn invert(&self, n:&Number)->Result<Self,Self::Error> { Ok(Self(n.0)) }
    fn encode(&self, e:&mut Encoder)->Result<(),CodecError> { e.write_u64(self.0 as u64) }
    fn decode(d:&mut Decoder<'_>)->Result<Self,CodecError> { Ok(Self(d.read_u64()? as i64)) }
}
fn history() -> Result<(), Box<dyn Error>> {
    let mut number=Number(0);
    let mut history=History::<Set,usize>::new(0,usize::MAX/4);
    let mut timeline=vec![(0i64,0usize)];
    let mut cursor=0;
    let mut random=0x9e3779b97f4a7c15u64;
    for i in 1..=10_000usize {
        random=random.wrapping_mul(6364136223846793005).wrapping_add(1);
        match random >> 61 {
            0|1 => { let expected=cursor>0; assert_eq!(history.undo(&mut number).unwrap(),expected);
                if expected {cursor-=1;} }
            2|3 => { let expected=cursor+1<timeline.len(); assert_eq!(history.redo(&mut number).unwrap(),expected);
                if expected {cursor+=1;} }
            _ => { let next=(random as i64)/1000;
                history.apply(&mut number,Transaction::new(vec![Set(next)])?,i,i as u64,EditKind::Other,512).unwrap();
                timeline.truncate(cursor+1); timeline.push((next,i)); cursor+=1; }
        }
        assert_eq!((number.0,*history.selection()),timeline[cursor]);
        assert_eq!(history.undo_len(),cursor);
        assert_eq!(history.redo_len(),timeline.len()-cursor-1);
    }
    println!("{{\"history_reference_steps\":10000}}");
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args:Vec<String>=std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("svg")=>svg(&args[2],&args[3]),
        Some("opaque")=>opaque(&args[2],&args[3]),
        Some("history")=>history(),
        _=>Err("expected svg, opaque or history".into()),
    }
}
