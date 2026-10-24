//! Transport icons from the design prototype, drawn with cairo in the widget's CSS color, so
//! hover and the accent apply as they do to text. Paths are in a 24×24 box with a 1.75 stroke.

use std::cell::Cell;
use std::f64::consts::{FRAC_PI_2, PI};
use std::rc::Rc;

use gtk::{cairo, prelude::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Shuffle,
    Previous,
    Play,
    Pause,
    Next,
    Repeat,
    /// Repeat with a small "1" inside.
    RepeatOne,
}

/// A drawn icon whose shape can change (play ↔ pause, repeat ↔ repeat one).
#[derive(Clone)]
pub struct IconView {
    pub area: gtk::DrawingArea,
    icon: Rc<Cell<Icon>>,
}

impl IconView {
    pub fn new(icon: Icon, size: i32) -> Self {
        let area = gtk::DrawingArea::new();
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_halign(gtk::Align::Center);
        area.set_valign(gtk::Align::Center);
        area.set_can_target(false);
        let icon = Rc::new(Cell::new(icon));
        let i = icon.clone();
        area.set_draw_func(move |a, cr, w, h| {
            let c = a.color();
            cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), c.alpha().into());
            let s = w.min(h) as f64 / 24.0;
            cr.translate((w as f64 - 24.0 * s) / 2.0, (h as f64 - 24.0 * s) / 2.0);
            cr.scale(s, s);
            cr.set_line_width(1.75);
            cr.set_line_cap(cairo::LineCap::Square);
            cr.set_line_join(cairo::LineJoin::Miter);
            draw(cr, i.get());
        });
        Self { area, icon }
    }

    pub fn set(&self, icon: Icon) {
        if self.icon.replace(icon) != icon {
            self.area.queue_draw();
        }
    }
}

fn polyline(cr: &cairo::Context, points: &[(f64, f64)]) {
    let (x, y) = points[0];
    cr.move_to(x, y);
    for &(x, y) in &points[1..] {
        cr.line_to(x, y);
    }
}

fn fill_polygon(cr: &cairo::Context, points: &[(f64, f64)]) {
    polyline(cr, points);
    cr.close_path();
    let _ = cr.fill();
}

fn draw(cr: &cairo::Context, icon: Icon) {
    match icon {
        Icon::Shuffle => {
            cr.move_to(3.0, 7.0);
            cr.line_to(6.5, 7.0);
            cr.curve_to(9.0, 7.0, 10.5, 8.5, 12.0, 12.0);
            cr.curve_to(13.5, 15.5, 15.0, 17.0, 17.5, 17.0);
            cr.line_to(21.0, 17.0);
            cr.move_to(3.0, 17.0);
            cr.line_to(6.5, 17.0);
            cr.curve_to(7.8, 17.0, 8.8, 16.6, 9.7, 15.8);
            cr.move_to(14.3, 8.2);
            cr.curve_to(15.2, 7.4, 16.2, 7.0, 17.5, 7.0);
            cr.line_to(21.0, 7.0);
            polyline(cr, &[(18.0, 4.0), (21.0, 7.0), (18.0, 10.0)]);
            polyline(cr, &[(18.0, 14.0), (21.0, 17.0), (18.0, 20.0)]);
            let _ = cr.stroke();
        }
        Icon::Previous => {
            polyline(cr, &[(6.0, 5.0), (6.0, 19.0)]);
            let _ = cr.stroke();
            fill_polygon(cr, &[(19.0, 5.0), (9.0, 12.0), (19.0, 19.0)]);
        }
        Icon::Next => {
            polyline(cr, &[(18.0, 5.0), (18.0, 19.0)]);
            let _ = cr.stroke();
            fill_polygon(cr, &[(5.0, 5.0), (15.0, 12.0), (5.0, 19.0)]);
        }
        Icon::Play => fill_polygon(cr, &[(8.0, 5.0), (20.0, 12.0), (8.0, 19.0)]),
        Icon::Pause => {
            cr.rectangle(7.0, 5.0, 3.0, 14.0);
            cr.rectangle(14.0, 5.0, 3.0, 14.0);
            let _ = cr.fill();
        }
        Icon::Repeat | Icon::RepeatOne => {
            cr.move_to(4.0, 11.0);
            cr.line_to(4.0, 9.0);
            cr.arc(6.0, 9.0, 2.0, PI, 1.5 * PI);
            cr.line_to(20.0, 7.0);
            polyline(cr, &[(17.0, 4.0), (20.0, 7.0), (17.0, 10.0)]);
            cr.move_to(20.0, 13.0);
            cr.line_to(20.0, 15.0);
            cr.arc(18.0, 15.0, 2.0, 0.0, FRAC_PI_2);
            cr.line_to(4.0, 17.0);
            polyline(cr, &[(7.0, 20.0), (4.0, 17.0), (7.0, 14.0)]);
            let _ = cr.stroke();
            if icon == Icon::RepeatOne {
                cr.set_line_width(1.6);
                polyline(cr, &[(11.0, 10.5), (12.5, 9.5), (12.5, 14.5)]);
                let _ = cr.stroke();
            }
        }
    }
}
