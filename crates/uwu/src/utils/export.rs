use crate::state::{CanvasObject, CanvasObjectOps, CanvasShapeType, CanvasState, StrokeWidth};
use egui::{Color32, Pos2, Rect};
use printpdf::{Color, Op, PdfDocument, Point, Pt, Rgb};
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn color_to_svg_attrs(color: Color32, prefix: &str) -> String {
    let r = color.r();
    let g = color.g();
    let b = color.b();
    let a = color.a() as f32 / 255.0;
    format!("{prefix}=\"rgb({r},{g},{b})\" {prefix}-opacity=\"{a}\"")
}

fn xml_escape(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            _ => s.push(c),
        }
    }
    s
}

pub fn export_page_to_svg(
    canvas: &CanvasState,
    canvas_color: Color32,
    path: &Path,
) -> std::io::Result<()> {
    let mut page_rect = Rect::NOTHING;
    for obj in &canvas.objects {
        match obj {
            CanvasObject::Image(_) => {}
            _ => {
                page_rect = page_rect.union(obj.bounding_box());
            }
        }
    }

    let padding = 20.0;
    let bbox = if page_rect.is_positive() {
        page_rect.expand(padding)
    } else {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))
    };

    let mut file = File::create(path)?;
    writeln!(
        file,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>"#
    )?;
    writeln!(
        file,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {} {}" width="{}" height="{}">"#,
        bbox.min.x,
        bbox.min.y,
        bbox.width(),
        bbox.height(),
        bbox.width(),
        bbox.height()
    )?;

    let bg_color_attr = color_to_svg_attrs(canvas_color, "fill");
    writeln!(
        file,
        r#"  <rect x="{}" y="{}" width="{}" height="{}" {} stroke="none" />"#,
        bbox.min.x,
        bbox.min.y,
        bbox.width(),
        bbox.height(),
        bg_color_attr
    )?;

    for obj in &canvas.objects {
        match obj {
            CanvasObject::Image(_) => {}
            CanvasObject::Stroke(stroke) => {
                if stroke.points.is_empty() {
                    continue;
                }
                let color_attr = color_to_svg_attrs(stroke.color, "stroke");
                let fill_color_attr = color_to_svg_attrs(stroke.color, "fill");

                if stroke.points.len() == 1 {
                    let p = stroke.points[0];
                    let r = stroke.width.first() / 2.0;
                    writeln!(
                        file,
                        r#"  <circle cx="{}" cy="{}" r="{}" {} />"#,
                        p.x, p.y, r, fill_color_attr
                    )?;
                } else {
                    match &stroke.width {
                        StrokeWidth::Fixed(w) => {
                            let mut d = String::new();
                            for (i, p) in stroke.points.iter().enumerate() {
                                if i == 0 {
                                    d.push_str(&format!("M {} {}", p.x, p.y));
                                } else {
                                    d.push_str(&format!(" L {} {}", p.x, p.y));
                                }
                            }
                            writeln!(
                                file,
                                r#"  <path d="{}" {} stroke-width="{}" fill="none" stroke-linecap="round" stroke-linejoin="round" />"#,
                                d, color_attr, w
                            )?;
                        }
                        StrokeWidth::Dynamic(widths) => {
                            for (i, (p1, p2)) in stroke
                                .points
                                .iter()
                                .zip(stroke.points[1..].iter())
                                .enumerate()
                            {
                                let w = (widths[i] + widths[i + 1]) / 2.0;
                                writeln!(
                                    file,
                                    r#"  <line x1="{}" y1="{}" x2="{}" y2="{}" {} stroke-width="{}" stroke-linecap="round" />"#,
                                    p1.x, p1.y, p2.x, p2.y, color_attr, w
                                )?;
                            }
                        }
                    }
                }
            }
            CanvasObject::Shape(shape) => {
                let color_attr = color_to_svg_attrs(shape.color, "stroke");
                let fill_attr = color_to_svg_attrs(shape.color, "fill");
                match shape.shape_type {
                    CanvasShapeType::Line => {
                        writeln!(
                            file,
                            r#"  <line x1="{}" y1="{}" x2="{}" y2="{}" {} stroke-width="2" stroke-linecap="round" />"#,
                            shape.pos.x,
                            shape.pos.y,
                            shape.pos.x + shape.size,
                            shape.pos.y,
                            color_attr
                        )?;
                    }
                    CanvasShapeType::Arrow => {
                        let p = shape.pos;
                        let size = shape.size;
                        let end_point = Pos2::new(p.x + size, p.y);
                        let arrow_size = size * 0.1;
                        let arrow_angle = std::f32::consts::PI / 6.0;
                        let arrow_point1 = Pos2::new(
                            end_point.x - arrow_size * arrow_angle.cos(),
                            end_point.y - arrow_size * arrow_angle.sin(),
                        );
                        let arrow_point2 = Pos2::new(
                            end_point.x - arrow_size * arrow_angle.cos(),
                            end_point.y + arrow_size * arrow_angle.sin(),
                        );
                        writeln!(
                            file,
                            r#"  <line x1="{}" y1="{}" x2="{}" y2="{}" {} stroke-width="2" stroke-linecap="round" />"#,
                            p.x, p.y, end_point.x, end_point.y, color_attr
                        )?;
                        writeln!(
                            file,
                            r#"  <line x1="{}" y1="{}" x2="{}" y2="{}" {} stroke-width="2" stroke-linecap="round" />"#,
                            end_point.x, end_point.y, arrow_point1.x, arrow_point1.y, color_attr
                        )?;
                        writeln!(
                            file,
                            r#"  <line x1="{}" y1="{}" x2="{}" y2="{}" {} stroke-width="2" stroke-linecap="round" />"#,
                            end_point.x, end_point.y, arrow_point2.x, arrow_point2.y, color_attr
                        )?;
                    }
                    CanvasShapeType::Rectangle => {
                        writeln!(
                            file,
                            r#"  <rect x="{}" y="{}" width="{}" height="{}" {} stroke-width="2" fill="none" />"#,
                            shape.pos.x, shape.pos.y, shape.size, shape.size, color_attr
                        )?;
                    }
                    CanvasShapeType::Triangle => {
                        let half_size = shape.size / 2.0;
                        let p1 = shape.pos;
                        let p2 = Pos2::new(p1.x + shape.size, p1.y);
                        let p3 = Pos2::new(p1.x + half_size, p1.y + half_size);
                        writeln!(
                            file,
                            r#"  <polygon points="{},{} {},{} {},{}" {} {} stroke-width="2" />"#,
                            p1.x, p1.y, p2.x, p2.y, p3.x, p3.y, fill_attr, color_attr
                        )?;
                    }
                    CanvasShapeType::Circle => {
                        writeln!(
                            file,
                            r#"  <circle cx="{}" cy="{}" r="{}" {} stroke-width="2" fill="none" />"#,
                            shape.pos.x,
                            shape.pos.y,
                            shape.size / 2.0,
                            color_attr
                        )?;
                    }
                }
            }
            CanvasObject::Text(text) => {
                let fill_attr = color_to_svg_attrs(text.color, "fill");
                let escaped = xml_escape(&text.text);
                writeln!(
                    file,
                    r#"  <text x="{}" y="{}" font-size="{}" {} font-family="sans-serif" dominant-baseline="hanging">{}</text>"#,
                    text.pos.x, text.pos.y, text.font_size, fill_attr, escaped
                )?;
            }
        }
    }

    writeln!(file, "</svg>")?;
    Ok(())
}

fn color_to_printpdf(color: Color32) -> Color {
    let r = color.r() as f32 / 255.0;
    let g = color.g() as f32 / 255.0;
    let b = color.b() as f32 / 255.0;
    Color::Rgb(Rgb::new(r, g, b, None))
}

pub fn export_all_pages_to_pdf(
    pages: &[&CanvasState],
    canvas_color: Color32,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if pages.is_empty() {
        return Ok(());
    }

    let mut doc = PdfDocument::new("Whiteboard Export");

    for (_page_idx, canvas) in pages.iter().enumerate() {
        let bbox = get_page_bbox(canvas);
        let width_pt = bbox.width();
        let height_pt = bbox.height();

        let width_mm: printpdf::Mm = Pt(width_pt).into();
        let height_mm: printpdf::Mm = Pt(height_pt).into();

        let mut ops = Vec::new();

        ops.push(Op::SetLineCapStyle {
            cap: printpdf::LineCapStyle::Round,
        });
        ops.push(Op::SetLineJoinStyle {
            join: printpdf::LineJoinStyle::Round,
        });

        // Draw page background as a filled polygon
        let pdf_bg_color = color_to_printpdf(canvas_color);
        ops.push(Op::SetFillColor { col: pdf_bg_color });
        let bg_points = vec![
            printpdf::LinePoint {
                p: Point {
                    x: Pt(0.0),
                    y: Pt(0.0),
                },
                bezier: false,
            },
            printpdf::LinePoint {
                p: Point {
                    x: Pt(width_pt),
                    y: Pt(0.0),
                },
                bezier: false,
            },
            printpdf::LinePoint {
                p: Point {
                    x: Pt(width_pt),
                    y: Pt(height_pt),
                },
                bezier: false,
            },
            printpdf::LinePoint {
                p: Point {
                    x: Pt(0.0),
                    y: Pt(height_pt),
                },
                bezier: false,
            },
        ];
        let bg_polygon = printpdf::Polygon {
            rings: vec![printpdf::PolygonRing { points: bg_points }],
            mode: printpdf::PaintMode::Fill,
            winding_order: printpdf::WindingOrder::NonZero,
        };
        ops.push(Op::DrawPolygon {
            polygon: bg_polygon,
        });

        for obj in &canvas.objects {
            match obj {
                CanvasObject::Image(_) => {}
                CanvasObject::Stroke(stroke) => {
                    if stroke.points.is_empty() {
                        continue;
                    }
                    let pdf_color = color_to_printpdf(stroke.color);
                    ops.push(Op::SetOutlineColor {
                        col: pdf_color.clone(),
                    });
                    ops.push(Op::SetFillColor { col: pdf_color });

                    if stroke.points.len() == 1 {
                        let p = stroke.points[0];
                        let r = stroke.width.first() / 2.0;
                        push_pdf_circle_ops(&mut ops, p, r, true, false, bbox);
                    } else {
                        match &stroke.width {
                            StrokeWidth::Fixed(w) => {
                                let points: Vec<printpdf::LinePoint> = stroke
                                    .points
                                    .iter()
                                    .map(|p| printpdf::LinePoint {
                                        p: Point {
                                            x: Pt(p.x - bbox.min.x),
                                            y: Pt(bbox.max.y - p.y),
                                        },
                                        bezier: false,
                                    })
                                    .collect();
                                ops.push(Op::SetOutlineThickness { pt: Pt(*w) });
                                ops.push(Op::DrawLine {
                                    line: printpdf::Line {
                                        points,
                                        is_closed: false,
                                    },
                                });
                            }
                            StrokeWidth::Dynamic(widths) => {
                                for (i, (p1, p2)) in stroke
                                    .points
                                    .iter()
                                    .zip(stroke.points[1..].iter())
                                    .enumerate()
                                {
                                    let w = (widths[i] + widths[i + 1]) / 2.0;
                                    let points = vec![
                                        printpdf::LinePoint {
                                            p: Point {
                                                x: Pt(p1.x - bbox.min.x),
                                                y: Pt(bbox.max.y - p1.y),
                                            },
                                            bezier: false,
                                        },
                                        printpdf::LinePoint {
                                            p: Point {
                                                x: Pt(p2.x - bbox.min.x),
                                                y: Pt(bbox.max.y - p2.y),
                                            },
                                            bezier: false,
                                        },
                                    ];
                                    ops.push(Op::SetOutlineThickness { pt: Pt(w) });
                                    ops.push(Op::DrawLine {
                                        line: printpdf::Line {
                                            points,
                                            is_closed: false,
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
                CanvasObject::Shape(shape) => {
                    let pdf_color = color_to_printpdf(shape.color);
                    ops.push(Op::SetOutlineColor {
                        col: pdf_color.clone(),
                    });
                    ops.push(Op::SetFillColor { col: pdf_color });
                    ops.push(Op::SetOutlineThickness { pt: Pt(2.0) });

                    match shape.shape_type {
                        CanvasShapeType::Line => {
                            let p1 = shape.pos;
                            let p2 = Pos2::new(p1.x + shape.size, p1.y);
                            let points = vec![
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p1.x - bbox.min.x),
                                        y: Pt(bbox.max.y - p1.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p2.x - bbox.min.x),
                                        y: Pt(bbox.max.y - p2.y),
                                    },
                                    bezier: false,
                                },
                            ];
                            ops.push(Op::DrawLine {
                                line: printpdf::Line {
                                    points,
                                    is_closed: false,
                                },
                            });
                        }
                        CanvasShapeType::Arrow => {
                            let p = shape.pos;
                            let size = shape.size;
                            let end_point = Pos2::new(p.x + size, p.y);
                            let arrow_size = size * 0.1;
                            let arrow_angle = std::f32::consts::PI / 6.0;
                            let arrow_point1 = Pos2::new(
                                end_point.x - arrow_size * arrow_angle.cos(),
                                end_point.y - arrow_size * arrow_angle.sin(),
                            );
                            let arrow_point2 = Pos2::new(
                                end_point.x - arrow_size * arrow_angle.cos(),
                                end_point.y + arrow_size * arrow_angle.sin(),
                            );

                            let points = vec![
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p.x - bbox.min.x),
                                        y: Pt(bbox.max.y - p.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(end_point.x - bbox.min.x),
                                        y: Pt(bbox.max.y - end_point.y),
                                    },
                                    bezier: false,
                                },
                            ];
                            ops.push(Op::DrawLine {
                                line: printpdf::Line {
                                    points,
                                    is_closed: false,
                                },
                            });

                            let points1 = vec![
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(end_point.x - bbox.min.x),
                                        y: Pt(bbox.max.y - end_point.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(arrow_point1.x - bbox.min.x),
                                        y: Pt(bbox.max.y - arrow_point1.y),
                                    },
                                    bezier: false,
                                },
                            ];
                            ops.push(Op::DrawLine {
                                line: printpdf::Line {
                                    points: points1,
                                    is_closed: false,
                                },
                            });

                            let points2 = vec![
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(end_point.x - bbox.min.x),
                                        y: Pt(bbox.max.y - end_point.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(arrow_point2.x - bbox.min.x),
                                        y: Pt(bbox.max.y - arrow_point2.y),
                                    },
                                    bezier: false,
                                },
                            ];
                            ops.push(Op::DrawLine {
                                line: printpdf::Line {
                                    points: points2,
                                    is_closed: false,
                                },
                            });
                        }
                        CanvasShapeType::Rectangle => {
                            let p = shape.pos;
                            let size = shape.size;
                            let points = vec![
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p.x - bbox.min.x),
                                        y: Pt(bbox.max.y - p.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p.x + size - bbox.min.x),
                                        y: Pt(bbox.max.y - p.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p.x + size - bbox.min.x),
                                        y: Pt(bbox.max.y - (p.y + size)),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p.x - bbox.min.x),
                                        y: Pt(bbox.max.y - (p.y + size)),
                                    },
                                    bezier: false,
                                },
                            ];
                            let polygon = printpdf::Polygon {
                                rings: vec![printpdf::PolygonRing { points }],
                                mode: printpdf::PaintMode::Stroke,
                                winding_order: printpdf::WindingOrder::NonZero,
                            };
                            ops.push(Op::DrawPolygon { polygon });
                        }
                        CanvasShapeType::Triangle => {
                            let half_size = shape.size / 2.0;
                            let p1 = shape.pos;
                            let p2 = Pos2::new(p1.x + shape.size, p1.y);
                            let p3 = Pos2::new(p1.x + half_size, p1.y + half_size);
                            let points = vec![
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p1.x - bbox.min.x),
                                        y: Pt(bbox.max.y - p1.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p2.x - bbox.min.x),
                                        y: Pt(bbox.max.y - p2.y),
                                    },
                                    bezier: false,
                                },
                                printpdf::LinePoint {
                                    p: Point {
                                        x: Pt(p3.x - bbox.min.x),
                                        y: Pt(bbox.max.y - p3.y),
                                    },
                                    bezier: false,
                                },
                            ];
                            let polygon = printpdf::Polygon {
                                rings: vec![printpdf::PolygonRing { points }],
                                mode: printpdf::PaintMode::FillStroke,
                                winding_order: printpdf::WindingOrder::NonZero,
                            };
                            ops.push(Op::DrawPolygon { polygon });
                        }
                        CanvasShapeType::Circle => {
                            push_pdf_circle_ops(
                                &mut ops,
                                shape.pos,
                                shape.size / 2.0,
                                false,
                                true,
                                bbox,
                            );
                        }
                    }
                }
                CanvasObject::Text(text) => {
                    let pdf_color = color_to_printpdf(text.color);
                    let text_y = text.pos.y + 0.8 * text.font_size;
                    let x_pt = Pt(text.pos.x - bbox.min.x);
                    let y_pt = Pt(bbox.max.y - text_y);

                    ops.push(Op::SetFont {
                        font: printpdf::PdfFontHandle::Builtin(printpdf::BuiltinFont::Helvetica),
                        size: Pt(text.font_size),
                    });
                    ops.push(Op::SetTextCursor {
                        pos: Point { x: x_pt, y: y_pt },
                    });
                    ops.push(Op::SetFillColor { col: pdf_color });
                    ops.push(Op::StartTextSection);
                    ops.push(Op::ShowText {
                        items: vec![printpdf::TextItem::Text(text.text.clone())],
                    });
                    ops.push(Op::EndTextSection);
                }
            }
        }

        doc.pages
            .push(printpdf::PdfPage::new(width_mm, height_mm, ops));
    }

    let bytes = doc.save(&printpdf::PdfSaveOptions::default(), &mut Vec::new());
    std::fs::write(path, bytes)?;
    Ok(())
}

fn get_page_bbox(canvas: &CanvasState) -> Rect {
    let mut page_rect = Rect::NOTHING;
    for obj in &canvas.objects {
        match obj {
            CanvasObject::Image(_) => {}
            _ => {
                page_rect = page_rect.union(obj.bounding_box());
            }
        }
    }
    let padding = 20.0;
    if page_rect.is_positive() {
        page_rect.expand(padding)
    } else {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))
    }
}

fn push_pdf_circle_ops(
    ops: &mut Vec<Op>,
    center: Pos2,
    radius: f32,
    has_fill: bool,
    has_stroke: bool,
    bbox: Rect,
) {
    let num_segments = 32;
    let mut points = Vec::new();
    for i in 0..num_segments {
        let angle = (i as f32 * 2.0 * std::f32::consts::PI) / num_segments as f32;
        let x = center.x + radius * angle.cos();
        let y = center.y + radius * angle.sin();
        points.push(printpdf::LinePoint {
            p: Point {
                x: Pt(x - bbox.min.x),
                y: Pt(bbox.max.y - y),
            },
            bezier: false,
        });
    }

    let mode = if has_fill && has_stroke {
        printpdf::PaintMode::FillStroke
    } else if has_fill {
        printpdf::PaintMode::Fill
    } else {
        printpdf::PaintMode::Stroke
    };

    let polygon = printpdf::Polygon {
        rings: vec![printpdf::PolygonRing { points }],
        mode,
        winding_order: printpdf::WindingOrder::NonZero,
    };
    ops.push(Op::DrawPolygon { polygon });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CanvasObject, CanvasShape, CanvasStroke, CanvasText};
    use egui::Color32;

    #[test]
    fn test_export_svg_and_pdf() {
        let mut canvas = CanvasState::default();

        // 1. Add a Stroke
        canvas.objects.push(CanvasObject::Stroke(CanvasStroke {
            points: vec![Pos2::new(10.0, 10.0), Pos2::new(50.0, 50.0)],
            width: StrokeWidth::Fixed(3.0),
            color: Color32::RED,
            base_width: 3.0,
            shape: None,
        }));

        // 2. Add a Shape (Rectangle)
        canvas.objects.push(CanvasObject::Shape(CanvasShape {
            shape_type: CanvasShapeType::Rectangle,
            pos: Pos2::new(100.0, 100.0),
            size: 50.0,
            color: Color32::GREEN,
        }));

        // 3. Add Text
        canvas.objects.push(CanvasObject::Text(CanvasText {
            text: "Hello World".to_string(),
            pos: Pos2::new(200.0, 200.0),
            color: Color32::BLUE,
            font_size: 24.0,
            cached_size: None,
        }));

        let temp_dir = std::env::temp_dir();
        let svg_path = temp_dir.join("test_output.svg");
        let pdf_path = temp_dir.join("test_output.pdf");

        // Test SVG export
        let res_svg = export_page_to_svg(&canvas, Color32::WHITE, &svg_path);
        assert!(res_svg.is_ok());
        assert!(svg_path.exists());
        let svg_data = std::fs::read_to_string(&svg_path).unwrap();
        assert!(svg_data.contains("<svg"));
        assert!(svg_data.contains("rect"));
        assert!(svg_data.contains("Hello World"));

        // Test PDF export
        let res_pdf = export_all_pages_to_pdf(&[&canvas], Color32::WHITE, &pdf_path);
        assert!(res_pdf.is_ok());
        assert!(pdf_path.exists());
        let pdf_data = std::fs::read(&pdf_path).unwrap();
        assert!(!pdf_data.is_empty());

        // Clean up
        let _ = std::fs::remove_file(svg_path);
        let _ = std::fs::remove_file(pdf_path);
    }
}
