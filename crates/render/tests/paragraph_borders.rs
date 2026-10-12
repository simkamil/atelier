// SPDX-License-Identifier: AGPL-3.0-or-later

use atelier_geom::Rect;
use atelier_render::paragraph_borders::paragraph_border_commands;
use atelier_render::{
    DisplayList, DisplayPath, PageInfo, PathElement, SvgImage, SvgLimits, SvgResourceProvider,
    render_svg,
};
use atelier_text_layout::borders::{
    BorderPagePlan, BorderPrimitive, LayoutPoint, LayoutRect, ParagraphBorderPlan,
};

struct NoResources;

impl SvgResourceProvider for NoResources {
    fn glyph_outline(&self, _font_id: u64, _glyph_id: u32) -> Option<&DisplayPath> {
        None
    }

    fn image(&self, _image_id: u64) -> Option<SvgImage<'_>> {
        None
    }
}

fn page() -> PageInfo {
    PageInfo::new(Rect::from_xywh(0.0, 0.0, 200.0, 200.0).unwrap()).unwrap()
}

fn plan(primitives: Vec<BorderPrimitive>) -> ParagraphBorderPlan {
    ParagraphBorderPlan {
        pages: vec![BorderPagePlan {
            page_index: 0,
            primitives,
        }],
    }
}

#[test]
fn adapted_commands_round_trip_and_render_as_svg_in_paint_order() {
    let plan = plan(vec![
        BorderPrimitive::Fill {
            rect: LayoutRect {
                x: 64,
                y: 128,
                width: 640,
                height: 320,
            },
            color: atelier_text_model::rich::Color { r: 255, g: 0, b: 0 },
        },
        BorderPrimitive::Stroke {
            start: LayoutPoint { x: 64, y: 128 },
            end: LayoutPoint { x: 704, y: 128 },
            width: 32,
            color: atelier_text_model::rich::Color { r: 0, g: 0, b: 0 },
        },
        BorderPrimitive::Dot {
            center: LayoutPoint { x: 384, y: 320 },
            diameter: 64,
            color: atelier_text_model::rich::Color { r: 0, g: 0, b: 255 },
        },
    ]);
    let commands = paragraph_border_commands(&plan, 0).unwrap();
    assert!(matches!(
        commands[0],
        atelier_render::Command::FillPath { .. }
    ));
    assert!(matches!(
        commands[1],
        atelier_render::Command::StrokePath { .. }
    ));
    assert!(matches!(
        commands[2],
        atelier_render::Command::FillPath { .. }
    ));

    let display = DisplayList::new(page(), commands).unwrap();
    let bytes = display.encode().unwrap();
    assert_eq!(DisplayList::decode(&bytes).unwrap(), display);
    let svg = render_svg(&display, &NoResources, SvgLimits::default()).unwrap();
    assert!(svg.contains("fill=\"rgb(100% 0% 0%)\""));
    assert!(svg.contains("stroke=\"rgb(0% 0% 0%)\""));
    assert!(svg.contains("stroke-width=\"0.5\""));
    assert!(svg.contains("fill=\"rgb(0% 0% 100%)\""));
    assert!(
        svg.find("fill=\"rgb(100% 0% 0%)\"").unwrap()
            < svg.find("stroke=\"rgb(0% 0% 0%)\"").unwrap()
    );
}

#[test]
fn circle_diameter_does_not_truncate_i64_and_geometry_errors_are_typed() {
    let huge = (1i64 << 32) + 64;
    let commands = paragraph_border_commands(
        &plan(vec![BorderPrimitive::Dot {
            center: LayoutPoint { x: 0, y: 0 },
            diameter: huge,
            color: atelier_text_model::rich::Color { r: 1, g: 2, b: 3 },
        }]),
        0,
    )
    .unwrap();
    let atelier_render::Command::FillPath { path, .. } = &commands[0] else {
        panic!("dot must be a filled circle path")
    };
    assert_eq!(path.elements.len(), 6);
    assert!(matches!(path.elements[1], PathElement::CubicTo { .. }));

    let invalid = plan(vec![BorderPrimitive::Dot {
        center: LayoutPoint { x: 0, y: 0 },
        diameter: 0,
        color: atelier_text_model::rich::Color { r: 0, g: 0, b: 0 },
    }]);
    assert!(paragraph_border_commands(&invalid, 0).is_err());
}

#[test]
fn absent_page_and_malformed_primitives_fail_without_returning_commands() {
    let plan = plan(vec![BorderPrimitive::Fill {
        rect: LayoutRect {
            x: 0,
            y: 0,
            width: -1,
            height: 10,
        },
        color: atelier_text_model::rich::Color { r: 0, g: 0, b: 0 },
    }]);
    assert!(paragraph_border_commands(&plan, 1).is_err());
    assert!(paragraph_border_commands(&plan, 0).is_err());
}

#[test]
fn fill_paths_use_nonzero_closed_rectangles() {
    let commands = paragraph_border_commands(
        &plan(vec![BorderPrimitive::Fill {
            rect: LayoutRect {
                x: 640,
                y: 1280,
                width: 3200,
                height: 640,
            },
            color: atelier_text_model::rich::Color {
                r: 10,
                g: 20,
                b: 30,
            },
        }]),
        0,
    )
    .unwrap();
    let atelier_render::Command::FillPath { path, color, .. } = &commands[0] else {
        panic!("expected fill path")
    };
    assert_eq!(
        (color.red, color.green, color.blue, color.alpha),
        (10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0)
    );
    assert_eq!(path.elements.len(), 5);
}
