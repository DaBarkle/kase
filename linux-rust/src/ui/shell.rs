//! The window's morphing body: a rounded slab drawn on a transparent window, the way the
//! island is drawn on its overlay surface.
//!
//! The window has no titlebar and a transparent surface a little larger than the body
//! (room for the body's own shadow and for it to grow). The body's size and corner radius
//! are animated by the window's springs; the content is laid out once at its full size and
//! only clipped by the body, so nothing reflows while the shape moves.

use crate::ui::style::Tokens;
use iced::advanced::Renderer as _;
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size, Theme, Vector,
};

/// Space round the body for its shadow (logical px, each side).
pub const MARGIN: f32 = 28.0;

/// What the body looks like this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// The content's laid-out size (the body at rest).
    pub inner: Size,
    /// The body's drawn size this frame, top-centred on `inner`.
    pub shown: Size,
    pub radius: f32,
    /// 0 = content fully visible, 1 = fully covered by the body's colour.
    pub cover: f32,
    /// 0..1: how strong the body's shadow is (it fades in as the body opens).
    pub lift: f32,
}

pub struct Morph<'a, M> {
    content: Element<'a, M>,
    body: Body,
}

pub fn morph<'a, M: 'a>(content: impl Into<Element<'a, M>>, body: Body) -> Element<'a, M> {
    Element::new(Morph {
        content: content.into(),
        body,
    })
}

impl<M> Morph<'_, M> {
    /// The body's rectangle inside `bounds`: top-centred, below the margin.
    fn shown_rect(&self, bounds: Rectangle) -> Rectangle {
        let s = self.body.shown;
        Rectangle {
            x: bounds.x + (bounds.width - s.width) / 2.0,
            y: bounds.y + MARGIN,
            width: s.width,
            height: s.height,
        }
    }
}

impl<M> Widget<M, Theme, Renderer> for Morph<'_, M> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let outer = limits.max();
        let inner = self.body.inner;
        let child = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, inner),
        );
        let child = child.move_to(Point::new((outer.width - inner.width) / 2.0, MARGIN));
        layout::Node::with_children(outer, vec![child])
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
        // Clicks outside the body (shadow, room to grow) reach nothing inside it.
        let rect = self.shown_rect(layout.bounds());
        let cursor = match cursor.position() {
            Some(p) if rect.contains(p) => cursor,
            _ => mouse::Cursor::Unavailable,
        };
        let child = layout.children().next().expect("morph has one child");
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
        let t = Tokens::of(theme);
        let rect = self.shown_rect(layout.bounds());
        if rect.width < 1.0 || rect.height < 1.0 {
            return;
        }
        let radius = self.body.radius.min(rect.height / 2.0).min(rect.width / 2.0);
        let edge = if t.dark {
            Color::from_rgba(1.0, 1.0, 1.0, 0.10)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.08)
        };
        let lift = self.body.lift.clamp(0.0, 1.0);
        renderer.fill_quad(
            renderer::Quad {
                bounds: rect,
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: radius.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.45 * lift),
                    offset: Vector::new(0.0, 8.0 * lift),
                    blur_radius: 24.0 * lift,
                },
                ..Default::default()
            },
            t.bg,
        );
        let child = layout.children().next().expect("morph has one child");
        // Children clip their text and layers to the viewport they are given, so the body's
        // rectangle is passed down as the viewport: nothing draws outside the shape.
        let clip = rect.intersection(viewport).unwrap_or(rect);
        // The clip is square: while the body is still a pill the content would show in its
        // corners, so it is only drawn once it starts to fade in (the body is near full size
        // by then, and the content's padding keeps it clear of the round corners).
        if self.body.cover >= 0.995 {
            return;
        }
        renderer.with_layer(clip, |r| {
            self.content.as_widget().draw(
                &tree.children[0],
                r,
                theme,
                style,
                child,
                cursor,
                &clip,
            );
        });
        if self.body.cover > 0.001 {
            // Its own layer, drawn after the content's (text draws above quads in a layer).
            renderer.with_layer(clip, |r| {
                r.fill_quad(
                    renderer::Quad {
                        bounds: rect,
                        border: Border {
                            radius: radius.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    Color {
                        a: self.body.cover.min(1.0),
                        ..t.bg
                    },
                );
            });
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
        let rect = self.shown_rect(layout.bounds());
        if !cursor.is_over(rect) {
            return mouse::Interaction::None;
        }
        let child = layout.children().next().expect("morph has one child");
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child,
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
        let child = layout.children().next().expect("morph has one child");
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], child, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let child = layout.children().next().expect("morph has one child");
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
        )
    }
}
