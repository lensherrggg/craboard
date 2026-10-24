use super::*;

pub(super) fn window_edges(app: &mut App, ui: &mut egui::Ui) {
    // 不能用 ui.max_rect()：视图内容（如四象限的底部内边距）会把根 Ui 撑大，
    // 热区跟着跑到窗口外；也不能用 viewport().inner_rect（屏幕坐标）。
    let full = ui.ctx().viewport_rect();
    let (l, t, r, b) = (full.left(), full.top(), full.right(), full.bottom());
    let handles = [
        (
            ResizeDir::new(true, false, true, false),
            Rect::from_min_max(Pos2::new(l, t), Pos2::new(l + CORNER, t + CORNER)),
        ),
        (
            ResizeDir::new(false, true, true, false),
            Rect::from_min_max(Pos2::new(r - CORNER, t), Pos2::new(r, t + CORNER)),
        ),
        (
            ResizeDir::new(true, false, false, true),
            Rect::from_min_max(Pos2::new(l, b - CORNER), Pos2::new(l + CORNER, b)),
        ),
        (
            ResizeDir::new(false, true, false, true),
            Rect::from_min_max(Pos2::new(r - CORNER, b - CORNER), Pos2::new(r, b)),
        ),
        (
            ResizeDir::new(true, false, false, false),
            Rect::from_min_max(Pos2::new(l, t + CORNER), Pos2::new(l + EDGE, b - CORNER)),
        ),
        (
            ResizeDir::new(false, true, false, false),
            Rect::from_min_max(Pos2::new(r - EDGE, t + CORNER), Pos2::new(r, b - CORNER)),
        ),
        (
            ResizeDir::new(false, false, true, false),
            Rect::from_min_max(Pos2::new(l + CORNER, t), Pos2::new(r - CORNER, t + EDGE)),
        ),
        (
            ResizeDir::new(false, false, false, true),
            Rect::from_min_max(Pos2::new(l + CORNER, b - EDGE), Pos2::new(r - CORNER, b)),
        ),
    ];

    let hovered = ui
        .ctx()
        .input(|i| i.pointer.hover_pos())
        .and_then(|p| handles.iter().find(|(_, rect)| rect.contains(p)))
        .map(|(dir, _)| *dir);
    // 四象限的网格会盖住底边，不能依赖控件的 drag_started；直接按指针位置启动缩放。
    let pressed = ui
        .ctx()
        .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    if app.resize_drag.is_none() && pressed {
        if let (Some(dir), Some(cursor)) = (hovered, win32::cursor_pos()) {
            if let Some(o) = ui.ctx().input(|i| i.viewport().outer_rect) {
                app.resize_drag = Some(ResizeDrag {
                    dir,
                    outer: o.min,
                    size: full.size(),
                    cursor,
                    ppp: ui.ctx().pixels_per_point(),
                });
                app.move_drag = None;
            }
        }
    }
    if app.resize_drag.is_some() && !ui.input(|i| i.pointer.any_down()) {
        app.resize_drag = None;
    }

    if let Some(rs) = app.resize_drag {
        ui.ctx().set_cursor_icon(rs.dir.cursor());
        app.move_drag = None;
        let Some(cursor) = win32::cursor_pos() else {
            return;
        };
        let d = egui::vec2(
            (cursor.0 - rs.cursor.0) / rs.ppp,
            (cursor.1 - rs.cursor.1) / rs.ppp,
        );
        let mut pos = rs.outer;
        let mut size = rs.size;
        if rs.dir.left {
            let w = (rs.size.x - d.x).max(MIN_W);
            pos.x = rs.outer.x + rs.size.x - w;
            size.x = w;
        }
        if rs.dir.right {
            size.x = (rs.size.x + d.x).max(MIN_W);
        }
        if rs.dir.top {
            let h = (rs.size.y - d.y).max(MIN_H);
            pos.y = rs.outer.y + rs.size.y - h;
            size.y = h;
        }
        if rs.dir.bottom {
            size.y = (rs.size.y + d.y).max(MIN_H);
        }
        let ctx = ui.ctx();
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        if rs.dir.left || rs.dir.top {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
    } else if let Some(dir) = hovered {
        ui.ctx().set_cursor_icon(dir.cursor());
    }
}

pub(super) fn grip_hint(ui: &mut egui::Ui) {
    let corner = ui.ctx().viewport_rect().max;
    ui.painter().add(Shape::convex_polygon(
        vec![
            corner - Vec2::new(4.0, 13.0),
            corner - Vec2::new(13.0, 4.0),
            corner - Vec2::new(4.0, 4.0),
        ],
        theme::active().line,
        Stroke::NONE,
    ));
}
