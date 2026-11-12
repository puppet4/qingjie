//! Native pixel composition and reusable edit surfaces, independent of window dispatch.
use super::*;
unsafe fn draw_mark(dc: HDC, m: &Mark, ox: i32, oy: i32) {
    if m.points.is_empty() {
        return;
    }
    let a = m.points[0];
    let b = *m.points.last().unwrap();
    let pen = CreatePen(PS_SOLID, m.width, m.color);
    let old = SelectObject(dc, pen);
    let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
    let r = Region::between(a, b, i32::MAX, i32::MAX);
    match m.tool {
        Tool::Pen => {
            MoveToEx(dc, a.x - ox, a.y - oy, std::ptr::null_mut());
            for p in &m.points {
                LineTo(dc, p.x - ox, p.y - oy);
            }
        }
        Tool::Rectangle => {
            Rectangle(dc, r.x - ox, r.y - oy, r.x + r.w - ox, r.y + r.h - oy);
        }
        Tool::Ellipse => {
            Ellipse(dc, r.x - ox, r.y - oy, r.x + r.w - ox, r.y + r.h - oy);
        }
        Tool::Arrow => {
            MoveToEx(dc, a.x - ox, a.y - oy, std::ptr::null_mut());
            LineTo(dc, b.x - ox, b.y - oy);
            let angle = ((b.y - a.y) as f64).atan2((b.x - a.x) as f64);
            for d in [-0.5f64, 0.5] {
                MoveToEx(dc, b.x - ox, b.y - oy, std::ptr::null_mut());
                LineTo(
                    dc,
                    b.x - ox - (18.0 * (angle + d).cos()) as i32,
                    b.y - oy - (18.0 * (angle + d).sin()) as i32,
                );
            }
        }
        Tool::Redact => fill(
            dc,
            Region {
                x: r.x - ox,
                y: r.y - oy,
                ..r
            },
            0x00202020,
        ),
        Tool::Text => {
            SetTextColor(dc, m.color);
            text(dc, a.x - ox, a.y - oy, &m.text);
        }
        Tool::Mosaic => {
            for y in (r.y..r.y + r.h).step_by(12) {
                for x in (r.x..r.x + r.w).step_by(12) {
                    let color = GetPixel(dc, x - ox, y - oy);
                    fill(
                        dc,
                        Region {
                            x: x - ox,
                            y: y - oy,
                            w: 12.min(r.x + r.w - x),
                            h: 12.min(r.y + r.h - y),
                        },
                        color,
                    );
                }
            }
        }
        _ => {}
    }
    SelectObject(dc, brush);
    SelectObject(dc, old);
    DeleteObject(pen);
}
unsafe fn font(dc: HDC) -> (HFONT, HGDIOBJ) {
    let f = CreateFontW(
        -18,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        1,
        0,
        0,
        5,
        0,
        wide("Microsoft YaHei UI").as_ptr(),
    );
    let old = SelectObject(dc, f);
    SetBkMode(dc, TRANSPARENT as i32);
    (f, old)
}
pub(super) unsafe fn output(a: &mut App) -> Result<Bitmap, String> {
    finish_text(a);
    let b = a.image.as_ref().ok_or("尚未截图")?;
    let r = a.selection;
    if !r.valid() {
        return Err("请先拖动选择截图区域".into());
    }
    let out = Bitmap::new(r.w, r.h)?;
    let composed = compose(&mut a.cache, b, &a.marks, None)?;
    if BitBlt(out.dc, 0, 0, r.w, r.h, composed.dc, r.x, r.y, SRCCOPY) == 0 {
        return Err("无法生成截图图像".into());
    }
    Ok(out)
}
fn union(a: Region, b: Region) -> Region {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Region {
        x,
        y,
        w: (a.x + a.w).max(b.x + b.w) - x,
        h: (a.y + a.h).max(b.y + b.h) - y,
    }
}
fn expand(r: Region, amount: i32) -> Region {
    Region {
        x: r.x - amount,
        y: r.y - amount,
        w: r.w + amount * 2,
        h: r.h + amount * 2,
    }
}
unsafe fn compose<'a>(
    cache: &'a mut RenderCache,
    source: &'a Bitmap,
    marks: &[Mark],
    active_pen: Option<&Mark>,
) -> Result<&'a Bitmap, String> {
    if marks.is_empty() && active_pen.is_none() {
        cache.composed = None;
        cache.committed = 0;
        return Ok(source);
    }
    if cache.composed.is_none() || cache.committed > marks.len() {
        let composed = Bitmap::new(source.w, source.h)?;
        BitBlt(
            composed.dc,
            0,
            0,
            source.w,
            source.h,
            source.dc,
            0,
            0,
            SRCCOPY,
        );
        cache.composed = Some(composed);
        cache.committed = 0;
        cache.active_points = 0;
    }
    let composed = cache.composed.as_ref().unwrap();
    if cache.committed < marks.len() {
        let (f, old) = font(composed.dc);
        for mark in &marks[cache.committed..] {
            draw_mark(composed.dc, mark, 0, 0);
        }
        SelectObject(composed.dc, old);
        DeleteObject(f);
        cache.committed = marks.len();
    }
    if let Some(mark) = active_pen {
        let from = cache.active_points.saturating_sub(1);
        if from < mark.points.len() && cache.active_points < mark.points.len() {
            let part = Mark {
                tool: Tool::Pen,
                points: mark.points[from..].to_vec(),
                color: mark.color,
                width: mark.width,
                text: String::new(),
            };
            draw_mark(composed.dc, &part, 0, 0);
            cache.active_points = mark.points.len();
        }
    }
    Ok(composed)
}
pub(super) unsafe fn render(a: &mut App) -> Result<&Bitmap, String> {
    let b = a.image.as_ref().ok_or("尚未截图")?;
    if a.pin {
        return Ok(b);
    }
    if a.cache.frame.is_none() {
        a.cache.frame = Some(Bitmap::new(b.w, b.h)?);
    }
    let buf = a.cache.frame.as_ref().unwrap();
    // Restore only the previous/current selection and chrome, at native resolution.
    let full = Region {
        x: 0,
        y: 0,
        w: b.w,
        h: b.h,
    };
    let dirty = if let Some(previous) = a.cache.previous_selection {
        let margin = dpi::scale(40, a.ui_dpi);
        let mut dirty = union(expand(previous, margin), expand(a.selection, margin));
        if a.bar.valid() {
            dirty = union(dirty, expand(a.bar, 8));
        }
        if !previous.valid() {
            let height = dpi::scale(90, a.ui_dpi);
            dirty = union(
                dirty,
                Region {
                    x: 0,
                    y: (b.h - height).max(0),
                    w: b.w,
                    h: height,
                },
            );
        }
        Region::between(
            Point {
                x: dirty.x,
                y: dirty.y,
            },
            Point {
                x: dirty.x + dirty.w,
                y: dirty.y + dirty.h,
            },
            b.w,
            b.h,
        )
    } else {
        full
    };
    BitBlt(
        buf.dc, dirty.x, dirty.y, dirty.w, dirty.h, b.dc, dirty.x, dirty.y, SRCCOPY,
    );
    let tint = Bitmap::new(1, 1)?;
    fill(
        tint.dc,
        Region {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        },
        ui::rgb(15, 23, 42),
    );
    GdiAlphaBlend(
        buf.dc,
        dirty.x,
        dirty.y,
        dirty.w,
        dirty.h,
        tint.dc,
        0,
        0,
        1,
        1,
        BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 58,
            AlphaFormat: 0,
        },
    );
    // Temporarily take the frame so the annotation cache can be updated separately.
    let buf = a.cache.frame.take().unwrap();
    let composed = match compose(
        &mut a.cache,
        b,
        &a.marks,
        a.active_mark.as_ref().filter(|m| m.tool == Tool::Pen),
    ) {
        Ok(scene) => scene,
        Err(e) => {
            a.cache.frame = Some(buf);
            return Err(e);
        }
    };
    let r = a.selection;
    if r.valid() {
        BitBlt(buf.dc, r.x, r.y, r.w, r.h, composed.dc, r.x, r.y, SRCCOPY);
        let saved = SaveDC(buf.dc);
        IntersectClipRect(buf.dc, r.x, r.y, r.x + r.w, r.y + r.h);
        let (f, old) = font(buf.dc);
        if let Some(m) = a.active_mark.as_ref().filter(|m| m.tool != Tool::Pen) {
            draw_mark(buf.dc, m, 0, 0);
        }
        SelectObject(buf.dc, old);
        DeleteObject(f);
        RestoreDC(buf.dc, saved);
        let saved = dpi::map(buf.dc, a.ui_dpi);
        let logical = |v| dpi::logical(v, a.ui_dpi);
        let lr = Region {
            x: logical(r.x),
            y: logical(r.y),
            w: logical(r.w),
            h: logical(r.h),
        };
        ui::selection(buf.dc, lr, logical(b.w), (r.w, r.h));
        if !a.drag {
            let toolbar = ui::toolbar(
                buf.dc,
                lr,
                logical(b.w),
                logical(b.h),
                a.tool as usize,
                a.color,
                a.hover,
                a.marks.is_empty(),
                a.redo.is_empty(),
                a.stroke_width,
            );
            let (bar, buttons) = match toolbar {
                Ok(value) => value,
                Err(e) => {
                    RestoreDC(buf.dc, saved);
                    a.cache.frame = Some(buf);
                    return Err(e);
                }
            };
            let physical = |r: Region| Region {
                x: dpi::scale(r.x, a.ui_dpi),
                y: dpi::scale(r.y, a.ui_dpi),
                w: dpi::scale(r.w, a.ui_dpi),
                h: dpi::scale(r.h, a.ui_dpi),
            };
            a.bar = physical(bar);
            a.buttons = buttons
                .into_iter()
                .map(|(id, r)| (id, physical(r)))
                .collect();
        } else {
            a.buttons.clear();
            a.bar = Region::default();
        }
        RestoreDC(buf.dc, saved);
    } else {
        a.bar = Region::default();
        a.buttons.clear();
        let saved = dpi::map(buf.dc, a.ui_dpi);
        let result = ui::welcome(
            buf.dc,
            dpi::logical(b.w, a.ui_dpi),
            dpi::logical(b.h, a.ui_dpi),
        );
        RestoreDC(buf.dc, saved);
        if let Err(e) = result {
            a.cache.frame = Some(buf);
            return Err(e);
        }
    }
    a.cache.previous_selection = Some(a.selection);
    a.cache.frame = Some(buf);
    Ok(a.cache.frame.as_ref().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn source() -> Bitmap {
        let b = Bitmap::new(1280, 820).unwrap();
        fill(
            b.dc,
            Region {
                x: 0,
                y: 0,
                w: 1280,
                h: 820,
            },
            ui::rgb(220, 230, 240),
        );
        fill(
            b.dc,
            Region {
                x: 640,
                y: 0,
                w: 640,
                h: 820,
            },
            ui::rgb(180, 210, 250),
        );
        b
    }
    #[test]
    fn incremental_pen_matches_fresh_scene_and_cancel_restores_pixels() {
        unsafe {
            let selection = Region {
                x: 100,
                y: 100,
                w: 900,
                h: 500,
            };
            let mut a = App {
                image: Some(source()),
                selection,
                drag: true,
                tool: Tool::Pen,
                active_mark: Some(Mark {
                    tool: Tool::Pen,
                    points: vec![Point { x: 200, y: 200 }; 2],
                    color: ui::rgb(240, 50, 50),
                    width: 5,
                    text: String::new(),
                }),
                ..App::default()
            };
            for point in [
                Point { x: 250, y: 220 },
                Point { x: 290, y: 270 },
                Point { x: 330, y: 200 },
            ] {
                a.active_mark.as_mut().unwrap().points.push(point);
                let actual = render(&mut a).unwrap().pixels().unwrap();
                let mut fresh = App {
                    image: Some(source()),
                    selection,
                    drag: true,
                    tool: Tool::Pen,
                    active_mark: a.active_mark.clone(),
                    ..App::default()
                };
                let expected = render(&mut fresh).unwrap().pixels().unwrap();
                assert!(actual == expected, "增量笔画必须与完整重绘一致");
            }
            a.undo();
            let actual = render(&mut a).unwrap().pixels().unwrap();
            let mut fresh = App {
                image: Some(source()),
                selection,
                tool: Tool::Pen,
                ..App::default()
            };
            assert!(
                actual == render(&mut fresh).unwrap().pixels().unwrap(),
                "取消笔画不能留下缓存痕迹"
            );
        }
    }
    #[test]
    fn cached_render_matches_fresh_render_after_editing() {
        unsafe {
            for dpi in [96, 144, 192] {
                let mut a = App {
                    image: Some(source()),
                    ui_dpi: dpi,
                    ..App::default()
                };
                let handle = render(&mut a).unwrap().handle;
                for (r, tool, hover) in [
                    (
                        Region {
                            x: 300,
                            y: 170,
                            w: 560,
                            h: 360,
                        },
                        Tool::Select,
                        None,
                    ),
                    (
                        Region {
                            x: 200,
                            y: 130,
                            w: 650,
                            h: 400,
                        },
                        Tool::Arrow,
                        Some(4),
                    ),
                    (
                        Region {
                            x: 20,
                            y: 25,
                            w: 300,
                            h: 200,
                        },
                        Tool::Pen,
                        None,
                    ),
                    (Region::default(), Tool::Select, None),
                ] {
                    a.selection = r;
                    a.tool = tool;
                    a.hover = hover;
                    let expected = App {
                        image: Some(source()),
                        selection: r,
                        tool,
                        hover,
                        ui_dpi: dpi,
                        ..App::default()
                    };
                    let mut expected = expected;
                    let frame = render(&mut a).unwrap();
                    assert_eq!(frame.handle, handle, "会话内应复用画布");
                    assert_eq!(
                        frame.pixels().unwrap(),
                        render(&mut expected).unwrap().pixels().unwrap(),
                        "缓存画面应与完整重绘一致：{dpi} DPI"
                    );
                }
            }
        }
    }
    #[test]
    fn crop_boundary_mosaic_matches_full_scene() {
        unsafe {
            let mark = Mark {
                tool: Tool::Mosaic,
                points: vec![Point { x: 8, y: 8 }, Point { x: 60, y: 60 }],
                color: 0,
                text: String::new(),
                width: 3,
            };
            let mut a = App {
                image: Some(source()),
                selection: Region {
                    x: 13,
                    y: 17,
                    w: 45,
                    h: 40,
                },
                marks: vec![mark],
                ..App::default()
            };
            let output = output(&mut a).unwrap();
            let scene = a.cache.composed.as_ref().unwrap();
            assert_eq!(GetPixel(output.dc, 0, 0), GetPixel(scene.dc, 13, 17));
        }
    }
}
