// SPDX-License-Identifier: AGPL-3.0-or-later
//! Emit one complete, deterministic ATDL fixture for the independent JS decoder.
use atelier_geom::{Point, Rect};
use atelier_render::{
    Color, Command, DisplayList, DisplayPath, DisplayTransform, FillRule, LineCap, LineJoin,
    PageInfo, PathElement, PositionedGlyph, StrokeStyle,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let page = PageInfo::new(Rect::from_xywh(0.0, 0.0, 240.0, 180.0)?)?;
    let path = DisplayPath::new(vec![
        PathElement::MoveTo(Point::try_new(4.0, 5.0)?),
        PathElement::LineTo(Point::try_new(30.0, 5.0)?),
        PathElement::QuadraticTo {
            control: Point::try_new(36.0, 12.0)?,
            to: Point::try_new(30.0, 20.0)?,
        },
        PathElement::CubicTo {
            control1: Point::try_new(24.0, 26.0)?,
            control2: Point::try_new(12.0, 26.0)?,
            to: Point::try_new(4.0, 20.0)?,
        },
        PathElement::Close,
        PathElement::Close,
        PathElement::LineTo(Point::try_new(8.0, 18.0)?),
        PathElement::Close,
    ])?;
    let color = Color::new(0.2, 0.4, 0.8, 0.75)?;
    let commands = vec![
        Command::FillPath {
            path: path.clone(),
            color,
            rule: FillRule::EvenOdd,
        },
        Command::StrokePath {
            path,
            color,
            style: StrokeStyle::new(2.5, LineCap::Round, LineJoin::Bevel, 3.0)?,
        },
        Command::DrawGlyphRun {
            font_id: 0xfedc_ba98_7654_3210,
            color,
            glyphs: vec![PositionedGlyph {
                glyph_id: u32::MAX,
                position: Point::try_new(50.0, 60.0)?,
                advance: Point::try_new(12.0, 0.0)?,
            }],
        },
        Command::DrawImage {
            image_id: 0x8000_0000_0000_0001,
            bounds: Rect::from_xywh(70.0, 40.0, 24.0, 18.0)?,
        },
        Command::PushClip {
            bounds: Rect::from_xywh(0.0, 0.0, 120.0, 120.0)?,
        },
        Command::PopClip,
        Command::PushTransform {
            transform: DisplayTransform::new(1.0, 0.25, 0.5, 1.0, 3.0, 4.0)?,
        },
        Command::PopTransform,
        Command::Link {
            bounds: Rect::from_xywh(2.0, 3.0, 10.0, 8.0)?,
            target: "\u{feff}https://example.test/é".to_owned(),
        },
        Command::Structure {
            role: "paragraph".to_owned(),
            node_id: 0xffff_ffff_ffff_fffe,
        },
    ];
    let bytes = DisplayList::new(page, commands)?.encode()?;
    for byte in bytes {
        print!("{byte:02x}");
    }
    println!();
    Ok(())
}
