//! Canvas-drawn pieces: battery ring gauges and the small line icons.

use crate::ui::format::{Gauge, ListeningMode};
use crate::ui::style::{self, Tokens};
use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke, path::Arc};
use iced::widget::{Canvas, column, text};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Renderer, Size, Theme, mouse};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// What moves on one gauge, from the window's springs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaugeMotion {
    /// Animated fill, 0..1 (None: draw the level as is). The number counts with it.
    pub fill: Option<f32>,
    /// Charging bolt scale (pops in with a bounce).
    pub bolt: f32,
    /// 0..1 through the green ripple that leaves the ring when charging starts (1: none).
    pub ripple: f32,
    /// 0..1 position of the light running round a charging arc (None: still).
    pub sheen: Option<f32>,
}

/// A faint light running along a charging arc filled to `frac`, `phase` (0..1) of the
/// way along it, brightest at its head (peak opacity `light`). Shared with the island.
pub fn draw_sheen(
    frame: &mut Frame,
    center: Point,
    radius: f32,
    frac: f32,
    phase: f32,
    width: f32,
    light: f32,
) {
    let start = -FRAC_PI_2;
    let head = start + TAU * frac * phase;
    let tail = (TAU * frac).min(0.9);
    for i in 0..6 {
        let k = i as f32 / 6.0;
        let a1 = head - tail * (1.0 - k - 1.0 / 6.0);
        if a1 <= start {
            continue;
        }
        let a0 = (head - tail * (1.0 - k)).max(start);
        let piece = Path::new(|b| {
            b.arc(Arc {
                center,
                radius,
                start_angle: Radians(a0),
                end_angle: Radians(a1),
            })
        });
        frame.stroke(
            &piece,
            Stroke::default()
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, light * k * k))
                .with_width(width * 0.6)
                .with_line_cap(LineCap::Round),
        );
    }
}

/// Room around the ring for its glow and the charging ripple.
const RING_ROOM: f32 = 9.0;

/// A circular battery gauge with its percentage inside.
#[derive(Debug, Clone, Copy)]
pub struct Ring {
    pub fraction: Option<f32>,
    /// The percentage shown (animated, may be fractional while it rolls); None shows "—".
    pub number: Option<f32>,
    /// Digits the settled number has, so the layout doesn't jump while it counts.
    pub digits: usize,
    pub dimmed: bool,
    pub tone: crate::ui::format::Tone,
    pub charging: bool,
    pub thickness: f32,
    /// Diameter of the ring box (the canvas adds [`RING_ROOM`] around it).
    pub size: f32,
    pub motion: GaugeMotion,
}

/// Odometer wheels for `v`, ones first: (digit at rest, 0..1 progress to the next digit).
/// A wheel only turns while the one below it rolls over from 9, like a car's odometer.
pub fn odometer(v: f32, digits: usize) -> Vec<(u32, f32)> {
    // Snap float noise (0.82 * 100) so settled numbers sit exactly on a digit.
    let v = ((v.max(0.0) * 1000.0).round() / 1000.0).min(999.0);
    (0..digits)
        .map(|k| {
            let unit = 10f32.powi(k as i32);
            let below = v % unit;
            let carry = (below - (unit - 1.0)).max(0.0);
            let w = (v / unit).floor() + if k == 0 { v.fract() } else { carry };
            let d = w.floor();
            (d as u32 % 10, w - d)
        })
        .collect()
}

impl<Message> canvas::Program<Message> for Ring {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let t = Tokens::of(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = self.size / 2.0 - self.thickness / 2.0 - 5.0;
        let m = self.motion;

        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default()
                .with_color(t.track)
                .with_width(self.thickness),
        );

        let muted = self.tone == crate::ui::format::Tone::Muted;
        let color = t.tone(self.tone, self.charging);
        let color = if muted {
            Color { a: 0.55, ..color }
        } else {
            color
        };
        let start = -FRAC_PI_2;
        let arc_to = |f: f32| {
            Path::new(|b| {
                b.arc(Arc {
                    center,
                    radius,
                    start_angle: Radians(start),
                    end_angle: Radians(start + TAU * f.max(0.004)),
                })
            })
        };
        if let Some(f) = self.fraction {
            let f = f.clamp(0.0, 1.0);
            if f > 0.0 {
                let arc = arc_to(f);
                if !muted {
                    // Soft edge light around the arc, as on the island.
                    let glow = if t.dark { 1.0 } else { 0.45 };
                    for (extra, a) in [(10.0, 0.06), (6.0, 0.10), (3.0, 0.16)] {
                        frame.stroke(
                            &arc,
                            Stroke::default()
                                .with_color(Color {
                                    a: a * glow,
                                    ..color
                                })
                                .with_width(self.thickness + extra)
                                .with_line_cap(LineCap::Round),
                        );
                    }
                }
                frame.stroke(
                    &arc,
                    Stroke::default()
                        .with_color(color)
                        .with_width(self.thickness)
                        .with_line_cap(LineCap::Round),
                );
                if let Some(phase) = m.sheen.filter(|_| f > 0.05) {
                    let light = if t.dark { 0.42 } else { 0.6 };
                    draw_sheen(&mut frame, center, radius, f, phase, self.thickness, light);
                }
            }
        }
        if m.ripple < 0.999 {
            // One ring of light leaving the gauge as charging starts.
            let r = m.ripple.clamp(0.0, 1.0);
            frame.stroke(
                &Path::circle(center, radius + self.thickness / 2.0 + 1.0 + RING_ROOM * r),
                Stroke::default()
                    .with_color(Color {
                        a: 0.55 * (1.0 - r),
                        ..t.green
                    })
                    .with_width(2.0 * (1.0 - r) + 0.5),
            );
        }

        self.draw_number(&mut frame, &t, center);
        if self.charging && m.bolt > 0.01 {
            // Bolt sits under the number so the percentage stays optically centred.
            let h = self.size * 0.16 * m.bolt.max(0.0);
            frame.fill(
                &bolt_path(center.x, center.y + self.size * 0.25, h),
                t.green,
            );
        }
        vec![frame.into_geometry()]
    }
}

impl Ring {
    /// The percentage as odometer wheels, with a smaller, quieter % so the number stays
    /// the hero.
    fn draw_number(&self, frame: &mut Frame, t: &Tokens, center: Point) {
        let size = (self.size * 0.22).round();
        let ink = if self.dimmed { t.text_dim } else { t.text };
        let text = |content: String, at: Point, size: f32, color: Color, font| canvas::Text {
            content,
            position: at,
            color,
            size: size.into(),
            font,
            align_x: iced::widget::text::Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        };
        let Some(v) = self.number else {
            frame.fill_text(text("—".into(), center, size, ink, style::INTER_SEMIBOLD));
            return;
        };
        let cell = size * 0.62;
        let sign = (size * 0.55).round();
        let sign_w = sign * 0.82;
        let n = self.digits.max(1);
        let width = n as f32 * cell + 1.0 + sign_w;
        let left = center.x - width / 2.0;
        let travel = size * 0.95;
        let wheels = odometer(v, n);
        let clip = Rectangle::new(
            Point::new(left - 2.0, center.y - size * 0.62),
            Size::new(n as f32 * cell + 4.0, size * 1.24),
        );
        frame.with_clip(clip, |f| {
            for (k, (d, p)) in wheels.iter().enumerate() {
                let x = left + (n - 1 - k) as f32 * cell + cell / 2.0;
                let y = center.y;
                // Leading zeros stay blank.
                let lead = k > 0 && v < 10f32.powi(k as i32);
                let a = Color {
                    a: ink.a * (1.0 - p),
                    ..ink
                };
                if !(lead && *d == 0) {
                    f.fill_text(text(
                        d.to_string(),
                        Point::new(x, y - travel * p),
                        size,
                        a,
                        style::INTER_SEMIBOLD,
                    ));
                }
                if *p > 0.001 {
                    let b = Color {
                        a: ink.a * p,
                        ..ink
                    };
                    let next = (d + 1) % 10;
                    f.fill_text(text(
                        next.to_string(),
                        Point::new(x, y + travel * (1.0 - p)),
                        size,
                        b,
                        style::INTER_SEMIBOLD,
                    ));
                }
            }
        });
        frame.fill_text(text(
            "%".into(),
            Point::new(
                left + n as f32 * cell + 1.0 + sign_w / 2.0,
                center.y + size * 0.12,
            ),
            sign,
            t.text_dim,
            style::INTER_MEDIUM,
        ));
    }
}

/// Small line icons drawn in the current text colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Glyph {
    Mode(ListeningMode),
    Bolt,
    Bluetooth,
    Earbuds,
}

/// Icon colour, resolved against the theme at draw time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ink {
    Text,
    Dim,
    Faint,
    Green,
    Accent,
}

impl Ink {
    pub fn resolve(self, t: &Tokens) -> Color {
        match self {
            Ink::Text => t.text,
            Ink::Dim => t.text_dim,
            Ink::Faint => t.text_faint,
            Ink::Green => t.green,
            Ink::Accent => t.accent,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Icon {
    pub glyph: Glyph,
    pub ink: Ink,
    pub alpha: f32,
}

fn bolt_path(cx: f32, cy: f32, h: f32) -> Path {
    // Lightning bolt inside a box of height h centred on (cx, cy).
    let s = h / 16.0;
    let p = |x: f32, y: f32| Point::new(cx + (x - 8.0) * s, cy + (y - 8.0) * s);
    Path::new(|b| {
        b.move_to(p(9.5, 0.0));
        b.line_to(p(2.5, 9.2));
        b.line_to(p(7.4, 9.2));
        b.line_to(p(6.3, 16.0));
        b.line_to(p(13.5, 6.6));
        b.line_to(p(8.6, 6.6));
        b.close();
    })
}

impl<Message> canvas::Program<Message> for Icon {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let base = self.ink.resolve(&Tokens::of(theme));
        let color = Color {
            a: base.a * self.alpha,
            ..base
        };
        let mut frame = Frame::new(renderer, bounds.size());
        let c = frame.center();
        let s = bounds.width.min(bounds.height);
        let w = (s / 12.0).max(1.4);
        let stroke = Stroke::default()
            .with_color(color)
            .with_width(w)
            .with_line_cap(LineCap::Round);
        let r = s * 0.36;
        match self.glyph {
            Glyph::Mode(ListeningMode::Off) => {
                frame.stroke(&Path::circle(c, r), stroke);
                let d = r * 0.7;
                frame.stroke(
                    &Path::line(Point::new(c.x - d, c.y + d), Point::new(c.x + d, c.y - d)),
                    stroke,
                );
            }
            Glyph::Mode(ListeningMode::Transparency) => {
                frame.fill(&Path::circle(c, r * 0.38), color);
                for i in 0..8 {
                    let a = i as f32 * TAU / 8.0;
                    let (sn, cs) = a.sin_cos();
                    frame.stroke(
                        &Path::line(
                            Point::new(c.x + cs * r * 0.72, c.y + sn * r * 0.72),
                            Point::new(c.x + cs * r * 1.12, c.y + sn * r * 1.12),
                        ),
                        stroke,
                    );
                }
            }
            Glyph::Mode(ListeningMode::Adaptive) => {
                frame.stroke(&Path::circle(c, r), stroke);
                let half = Path::new(|b| {
                    b.arc(Arc {
                        center: c,
                        radius: r,
                        start_angle: Radians(FRAC_PI_2),
                        end_angle: Radians(FRAC_PI_2 + PI),
                    });
                    b.close();
                });
                frame.fill(&half, color);
            }
            Glyph::Mode(ListeningMode::NoiseCancellation) => {
                frame.fill(&Path::circle(c, r * 0.62), color);
                frame.stroke(&Path::circle(c, r * 1.05), stroke);
            }
            Glyph::Bolt => {
                frame.fill(&bolt_path(c.x, c.y, s * 0.95), color);
            }
            Glyph::Bluetooth => {
                let h = s * 0.34;
                let x = c.x - s * 0.02;
                let path = Path::new(|b| {
                    b.move_to(Point::new(x - h * 0.55, c.y - h * 0.5));
                    b.line_to(Point::new(x + h * 0.55, c.y + h * 0.5));
                    b.line_to(Point::new(x, c.y + h));
                    b.line_to(Point::new(x, c.y - h));
                    b.line_to(Point::new(x + h * 0.55, c.y - h * 0.5));
                    b.line_to(Point::new(x - h * 0.55, c.y + h * 0.5));
                });
                frame.stroke(&path, stroke.with_line_join(canvas::LineJoin::Round));
            }
            Glyph::Earbuds => {
                // The app's mark, an AirPods case: same 32-unit geometry as the app icon,
                // the tray icon and the overlay glyph. The status light is an even-odd hole.
                let u = s / 32.0;
                let at = |x: f32, y: f32| Point::new(c.x + (x - 16.0) * u, c.y + (y - 16.0) * u);
                let lid = Path::new(|b| {
                    b.rounded_rectangle(
                        at(4.0, 6.0),
                        Size::new(24.0 * u, 6.3 * u),
                        iced::border::Radius {
                            top_left: 6.3 * u,
                            top_right: 6.3 * u,
                            bottom_right: 0.0,
                            bottom_left: 0.0,
                        },
                    )
                });
                let body = Path::new(|b| {
                    b.rounded_rectangle(
                        at(4.0, 14.1),
                        Size::new(24.0 * u, 11.9 * u),
                        iced::border::Radius {
                            top_left: 0.0,
                            top_right: 0.0,
                            bottom_right: 7.0 * u,
                            bottom_left: 7.0 * u,
                        },
                    );
                    b.circle(at(16.0, 19.6), 1.7 * u);
                });
                frame.fill(&lid, color);
                frame.fill(
                    &body,
                    canvas::Fill {
                        style: canvas::Style::Solid(color),
                        rule: canvas::fill::Rule::EvenOdd,
                    },
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

pub fn icon<'a, M: 'a>(glyph: Glyph, size: f32, ink: Ink) -> Element<'a, M> {
    icon_alpha(glyph, size, ink, 1.0)
}

pub fn icon_alpha<'a, M: 'a>(glyph: Glyph, size: f32, ink: Ink, alpha: f32) -> Element<'a, M> {
    Canvas::new(Icon { glyph, ink, alpha })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

/// Ring with the percentage in the middle, label (and optional caption) underneath.
pub fn gauge_view<'a, M: 'a>(g: &Gauge, size: f32, motion: GaugeMotion) -> Element<'a, M> {
    let dimmed = g.dimmed;
    let fraction = g
        .level
        .map(|l| motion.fill.unwrap_or(l.min(100) as f32 / 100.0));
    let ring = Canvas::new(Ring {
        fraction,
        number: fraction.map(|f| f * 100.0),
        digits: g.level.map_or(1, |l| l.min(100).to_string().len()),
        dimmed,
        tone: g.tone,
        charging: g.charging,
        thickness: (size / 12.5).round().max(4.0),
        size,
        motion,
    })
    .width(Length::Fill)
    .height(Length::Fixed(size + 2.0 * RING_ROOM));

    let label = text(g.label)
        .size(13)
        .font(style::INTER_MEDIUM)
        .style(move |theme: &Theme| {
            let t = Tokens::of(theme);
            text::Style {
                color: Some(if dimmed { t.text_faint } else { t.text_dim }),
            }
        });

    let mut col = column![ring, label]
        .spacing(0)
        .width(Length::Fill)
        .align_x(iced::Center);

    if let Some(c) = &g.caption {
        col = col.push(
            // Two short lines fit under a ring better than one long one.
            text(c.replacen("seen ", "seen\n", 1))
                .size(11)
                .line_height(1.25)
                .width(Length::Fill)
                .align_x(iced::Center)
                .style(|theme: &Theme| text::Style {
                    color: Some(Tokens::of(theme).text_faint),
                }),
        );
    }
    col.into()
}

/// Press feedback: down to 97% and back, a little springy.
const PRESS: crate::ui::motion::Curve = crate::ui::motion::Curve {
    response: 0.26,
    damping: 0.6,
};

/// Wraps content to draw it shifted by `offset` and covered by the page background at
/// `cover` (0 = fully visible), for page transitions, and optionally to scale it to 97%
/// while the mouse is held on it. Layout and input are the content's own; it only changes
/// how the content is drawn, and asks for frames only while the press spring moves.
pub struct Moved<'a, M> {
    content: Element<'a, M>,
    offset: iced::Vector,
    cover: f32,
    press: bool,
}

struct PressState {
    spring: crate::ui::motion::Spring,
    down: bool,
}

pub fn moved<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    offset: iced::Vector,
    cover: f32,
) -> Element<'a, M> {
    Element::new(Moved {
        content: content.into(),
        offset,
        cover,
        press: false,
    })
}

/// Content that dips to 97% while pressed (off with Reduce motion).
pub fn pressable<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Moved {
        content: content.into(),
        offset: iced::Vector::ZERO,
        cover: 0.0,
        press: !crate::ui::motion::reduced_ui(),
    })
}

mod moved_impl {
    use super::*;
    use iced::advanced::Renderer as _;
    use iced::advanced::widget::{Operation, Tree, tree};
    use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, overlay, renderer};
    use iced::{Event, Transformation, Vector};
    use std::time::Instant;

    impl<M> Widget<M, Theme, Renderer> for Moved<'_, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<PressState>()
        }

        fn state(&self) -> tree::State {
            tree::State::new(PressState {
                spring: crate::ui::motion::Spring::new(1.0, PRESS, Instant::now()),
                down: false,
            })
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> iced::Size<Length> {
            self.content.as_widget().size()
        }

        fn size_hint(&self) -> iced::Size<Length> {
            self.content.as_widget().size_hint()
        }

        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        }

        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
            if !self.press {
                return;
            }
            let st = tree.state.downcast_mut::<PressState>();
            let now = Instant::now();
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    if cursor.is_over(layout.bounds()) =>
                {
                    st.down = true;
                    st.spring.retarget(0.97, now);
                    shell.request_redraw();
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if st.down => {
                    st.down = false;
                    st.spring.retarget(1.0, now);
                    shell.request_redraw();
                }
                Event::Window(iced::window::Event::RedrawRequested(at))
                    if !st.spring.at_rest(*at) =>
                {
                    shell.request_redraw();
                }
                _ => {}
            }
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            let b = layout.bounds();
            let scale = tree
                .state
                .downcast_ref::<PressState>()
                .spring
                .value(Instant::now());
            let draw = |r: &mut Renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    r,
                    theme,
                    style,
                    layout,
                    cursor,
                    viewport,
                )
            };
            if self.offset == Vector::ZERO && scale == 1.0 {
                draw(renderer);
            } else {
                let c = b.center();
                let t = Transformation::translate(c.x + self.offset.x, c.y + self.offset.y)
                    * Transformation::scale(scale)
                    * Transformation::translate(-c.x, -c.y);
                renderer.with_transformation(t, draw);
            }
            if self.cover > 0.001 {
                let bg = Tokens::of(theme).bg;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: b,
                        ..Default::default()
                    },
                    Color {
                        a: self.cover.min(1.0),
                        ..bg
                    },
                );
            }
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }

        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
