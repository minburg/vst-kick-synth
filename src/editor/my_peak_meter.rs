//! A super simple peak meter widget.

use nih_plug::prelude::util;
use std::cell::Cell;
use std::time::Duration;
use std::time::Instant;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;

/// The thickness of a tick inside of the peak meter's bar.
const TICK_WIDTH: f32 = 1.0;
/// The gap between individual ticks.
const TICK_GAP: f32 = 1.0;

/// The decibel value corresponding to the very left of the bar.
const MIN_TICK: f32 = -90.0;
/// The decibel value corresponding to the very right of the bar.
const MAX_TICK: f32 = 20.0;

/// A simple horizontal peak meter.
///
/// TODO: There are currently no styling options at all
/// TODO: Vertical peak meter, this is just a proof of concept to fit the gain GUI example.
pub struct MyPeakMeter;

/// The bar bit for the peak meter, manually drawn using vertical lines.
struct MyPeakMeterBar<L, P>
where
    L: Lens<Target = f32>,
    P: Lens<Target = f32>,
{
    level_dbfs: L,
    peak_dbfs: P,
    flip_horizontal: bool,
}

impl MyPeakMeter {
    /// Creates a new [`MyPeakMeter`] for the given value in decibel, optionally holding the peak
    /// value for a certain amount of time.
    pub fn new<L>(cx: &mut Context, level_dbfs: L, hold_time: Option<Duration>, flip_horizontal: bool) -> Handle<'_, Self>
    where
        L: Lens<Target = f32>,
    {
        Self.build(cx, |cx| {
            // Now for something that may be illegal under some jurisdictions. If a hold time is
            // given, then we'll build a new lens that always gives the held peak level for the
            // current moment in time by mutating some values captured into the mapping closure.
            let held_peak_value_db = Cell::new(f32::MIN);
            let last_held_peak_value: Cell<Option<Instant>> = Cell::new(None);
            let peak_dbfs = level_dbfs.map(move |level| -> f32 {
                match hold_time {
                    Some(hold_time) => {
                        let mut peak_level = held_peak_value_db.get();
                        let peak_time = last_held_peak_value.get();

                        let now = Instant::now();
                        if *level >= peak_level
                            || peak_time.is_none()
                            || now > peak_time.unwrap() + hold_time
                        {
                            peak_level = *level;
                            held_peak_value_db.set(peak_level);
                            last_held_peak_value.set(Some(now));
                        }

                        peak_level
                    }
                    None => util::MINUS_INFINITY_DB,
                }
            });

            MyPeakMeterBar {
                level_dbfs,
                peak_dbfs,
                flip_horizontal,
            }
                .build(cx, |_| {})
                .class("bar");
        })
    }
}

impl View for MyPeakMeter {
    fn element(&self) -> Option<&'static str> {
        Some("my-peak-meter")
    }
}

impl<L, P> View for MyPeakMeterBar<L, P>
where
    L: Lens<Target = f32>,
    P: Lens<Target = f32>,
{
    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let level_dbfs = self.level_dbfs.get(cx);
        let peak_dbfs = self.peak_dbfs.get(cx);

        // These basics are taken directly from the default implementation of this function
        let bounds = cx.bounds();
        if bounds.w == 0.0 || bounds.h == 0.0 {
            return;
        }

        // TODO: It would be cool to allow the text color property to control the gradient here. For
        //       now we'll only support basic background colors and borders.
        let background_color = cx.background_color();
        let border_color = cx.border_color();
        let opacity = cx.opacity();
        let mut background_color: vg::Color = background_color.into();
        background_color.set_alphaf(background_color.a * opacity);
        let mut border_color: vg::Color = border_color.into();
        border_color.set_alphaf(border_color.a * opacity);
        let border_width = cx.border_width();

        let mut path = vg::Path::new();
        {
            let x = bounds.x + border_width / 2.0;
            let y = bounds.y + border_width / 2.0;
            let w = bounds.w - border_width;
            let h = bounds.h - border_width;
            path.move_to(x, y);
            path.line_to(x, y + h);
            path.line_to(x + w, y + h);
            path.line_to(x + w, y);
            path.line_to(x, y);
            path.close();
        }

        // Fill with background color
        let paint = vg::Paint::color(background_color);
        canvas.fill_path(&path, &paint);

        // And now for the fun stuff. We'll try to not overlap the border, but we'll draw that last
        // just in case.
        let bar_bounds = bounds.shrink(border_width / 2.0);
        let bar_ticks_start_x = bar_bounds.left().floor() as i32;
        let bar_ticks_end_x = bar_bounds.right().ceil() as i32;

        // NOTE: We'll scale this with the nearest integer DPI ratio. That way it will still look
        //       good at 2x scaling, and it won't look blurry at 1.x times scaling.
        let dpi_scale = cx.logical_to_physical(1.0).floor().max(1.0);
        let bar_tick_coordinates = (bar_ticks_start_x..bar_ticks_end_x)
            .step_by(((TICK_WIDTH + TICK_GAP) * dpi_scale).round() as usize);
        for tick_x in bar_tick_coordinates {
            let tick_fraction =
                (tick_x - bar_ticks_start_x) as f32 / (bar_ticks_end_x - bar_ticks_start_x) as f32;

            let display_fraction = if self.flip_horizontal { 1.0 - tick_fraction } else { tick_fraction };
            let tick_db = (display_fraction * (MAX_TICK - MIN_TICK)) + MIN_TICK;

            if tick_db > level_dbfs {
                if self.flip_horizontal {
                    continue;
                } else {
                    break;
                }
            }

            // femtovg draws paths centered on these coordinates, so in order to be pixel perfect we
            // need to account for that. Otherwise the ticks will be 2px wide instead of 1px.
            let mut path = vg::Path::new();
            path.move_to(tick_x as f32 + (dpi_scale / 0.4), bar_bounds.top());
            path.line_to(tick_x as f32 + (dpi_scale / 2.0), bar_bounds.bottom());


            let start = Color::rgba(27,16,32,100);
            // this is the end color of the peak meter
            let end = Color::rgba(100,73,113,100);

            // 1. Convert u8 to f32 and normalize to 0.0...1.0 range
            let start_r = start.r() as f32 / 255.0;
            let start_g = start.g() as f32 / 255.0;
            let start_b = start.b() as f32 / 255.0;

            let end_r = end.r() as f32 / 255.0;
            let end_g = end.g() as f32 / 255.0;
            let end_b = end.b() as f32 / 255.0;

            // 2. Now you can safely multiply by the f32 tick_fraction
            let r = start_r + (end_r - start_r) * display_fraction;
            let g = start_g + (end_g - start_g) * display_fraction;
            let b = start_b + (end_b - start_b) * display_fraction;

            // 3. Create the paint (rgbaf expects 0.0 to 1.0)
            let mut paint = vg::Paint::color(vg::Color::rgbaf(r, g, b, opacity));

            paint.set_line_width(TICK_WIDTH * dpi_scale);
            canvas.stroke_path(&path, &paint);
        }

        // Draw the hold peak value if the hold time option has been set
        let db_to_x_coord = |db: f32| {
            let tick_fraction = ((db - MIN_TICK) / (MAX_TICK - MIN_TICK)).clamp(0.0, 1.0);
            let x_offset = (bar_ticks_end_x - bar_ticks_start_x) as f32 * tick_fraction;

            if self.flip_horizontal {
                (bar_ticks_end_x as f32 - x_offset).round()
            } else {
                (bar_ticks_start_x as f32 + x_offset).round()
            }
        };
        if (MIN_TICK..MAX_TICK).contains(&peak_dbfs) {
            // femtovg draws paths centered on these coordinates, so in order to be pixel perfect we
            // need to account for that. Otherwise the ticks will be 2px wide instead of 1px.
            let peak_x = db_to_x_coord(peak_dbfs);
            let mut path = vg::Path::new();
            path.move_to(peak_x + (dpi_scale / 2.0), bar_bounds.top());
            path.line_to(peak_x + (dpi_scale / 2.0), bar_bounds.bottom());

            // this is peak visualization color of the peak meter
            let mut paint = vg::Paint::color(vg::Color::rgbaf(0.4, 0.2, 0.4, opacity));
            paint.set_line_width(TICK_WIDTH * dpi_scale);
            canvas.stroke_path(&path, &paint);
        }

        // Draw border last
        let mut paint = vg::Paint::color(border_color);
        paint.set_line_width(border_width);
        canvas.stroke_path(&path, &paint);
    }
}
