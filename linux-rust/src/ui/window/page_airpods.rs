//! The AirPods tab: battery hero, status island and listening mode controls.

use super::*;
use iced::widget::{column, row};

const PILL_TEXT: f32 = 13.0;

/// One status line: light, headline and the quieter detail, at `alpha`.
fn status_line<'a>(s: &format::Status, alpha: f32) -> Element<'a, Message> {
    let mut line = row![
        container(Space::new())
            .width(7)
            .height(7)
            .style(style::status_dot(s.tone, alpha)),
        text(s.headline.clone())
            .size(PILL_TEXT)
            .font(style::INTER_SEMIBOLD)
            .wrapping(text::Wrapping::None)
            .style(style::status_headline(alpha)),
    ]
    .spacing(8)
    .align_y(Center);
    if let Some(detail) = &s.detail {
        line = line.push(
            text(format!("· {detail}"))
                .size(PILL_TEXT)
                .font(style::INTER_MEDIUM)
                .wrapping(text::Wrapping::None)
                .style(style::status_detail(s.tone, alpha)),
        );
    }
    line.into()
}

/// Laid-out width of one line of text, as iced will draw it.
fn text_width(s: &str, font: iced::Font) -> f32 {
    use iced::advanced::text::Paragraph as _;
    type P = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;
    P::with_text(iced::advanced::Text {
        content: s,
        bounds: Size::INFINITE,
        size: PILL_TEXT.into(),
        line_height: text::LineHeight::default(),
        font,
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    })
    .min_width()
}

/// The status island's width for `s`: padding, light, headline and detail.
pub(super) fn pill_width(s: &format::Status) -> f32 {
    let mut w = 14.0 + 7.0 + 8.0 + text_width(&s.headline, style::INTER_SEMIBOLD) + 16.0;
    if let Some(d) = &s.detail {
        w += 8.0 + text_width(&format!("· {d}"), style::INTER_MEDIUM);
    }
    w.ceil() + 1.0
}

impl App {
    pub(super) fn airpods_tab(&self) -> Element<'_, Message> {
        match (self.snap.adapter, self.primary()) {
            (AdapterState::Off, _) => empty_state(
                Glyph::Bluetooth,
                "Bluetooth is off",
                "Turn on Bluetooth from the system tray or System Settings › Bluetooth. \
                 Kase reconnects on its own.",
            ),
            (AdapterState::Missing, None) if self.started.elapsed() < ADAPTER_GRACE => {
                empty_state(Glyph::Bluetooth, "Looking for Bluetooth…", "One moment.")
            }
            (AdapterState::Missing, None) => empty_state(
                Glyph::Bluetooth,
                "Bluetooth unavailable",
                "No Bluetooth adapter was found. Check that Bluetooth is enabled \
                 in System Settings and the adapter is plugged in.",
            ),
            (_, None) => empty_state(
                Glyph::Earbuds,
                "No AirPods yet",
                "Pair your AirPods in System Settings › Bluetooth. \
                 Kase finds them automatically — nothing to set up here.",
            ),
            (_, Some(d)) => {
                let mut page = column![self.hero(d)].spacing(0);
                if d.in_use_elsewhere() {
                    page = page.push(gap(18.0)).push(self.elsewhere_banner(d));
                }
                if let Some(controls) = self.controls(d) {
                    page = page.push(controls);
                } else if !d.is_connected() {
                    page = page.push(gap(22.0)).push(away_hint(d));
                }
                page.into()
            }
        }
    }

    pub(super) fn hero<'a>(&self, d: &'a DeviceSnapshot) -> Element<'a, Message> {
        let mut subtitle = format::model_name(d.model).to_string();
        if let (true, Some(audio)) = (d.is_connected(), d.audio.as_deref()) {
            subtitle.push_str(" · ");
            subtitle.push_str(audio);
        }
        if d.is_connected() && !d.in_use_elsewhere() && d.other_devices > 0 {
            subtitle.push_str(&match d.other_devices {
                1 => " · Also on 1 other device".to_string(),
                n => format!(" · Also on {n} other devices"),
            });
        }

        let gauges = format::gauges(d, self.now);
        let size = if gauges.len() == 1 { 132.0 } else { 104.0 };
        let mut rings = row![].spacing(4).align_y(iced::Alignment::Start);
        for (i, g) in gauges.iter().enumerate() {
            let m = self.gauge_motion(i, g);
            rings = rings.push(container(widgets::gauge_view::<Message>(g, size, m)).width(Fill));
        }
        // AirPods 4: the case-open animation between the status island and the name. The
        // box keeps its size before the first frame is decoded, so nothing moves.
        let art: Element<'a, Message> = if has_art(d) {
            let size = Length::Fixed(ART);
            match self.art.current() {
                Some(h) => iced::widget::image(h.clone())
                    .width(size)
                    .height(size)
                    .into(),
                None => Space::new().width(size).height(size).into(),
            }
        } else {
            Space::new().height(0).into()
        };
        column![
            self.status_island(d),
            gap(18.0),
            art,
            text(d.name.clone())
                .size(26)
                .font(style::INTER_SEMIBOLD)
                .align_x(Center)
                .wrapping(text::Wrapping::WordOrGlyph),
            gap(4.0),
            text(subtitle).size(13).style(dim_text).align_x(Center),
            gap(12.0),
            rings,
        ]
        .align_x(Center)
        .width(Fill)
        .into()
    }

    /// The window's own little Dynamic Island: what matters right now, with an edge light
    /// that pulses when it changes. Its width springs to fit new text while the old text
    /// and colour cross-fade into the new.
    pub(super) fn status_island<'a>(&self, d: &DeviceSnapshot) -> Element<'a, Message> {
        let s = format::status(d, self.now);
        let fade = self.anim(Anim::PillFade, 1.0).clamp(0.0, 1.0);
        let from = self.prev_status.as_ref().filter(|_| fade < 1.0);
        let mut lines = stack![];
        if let Some(p) = from {
            lines = lines.push(status_line(p, 1.0 - fade));
        }
        lines = lines.push(status_line(&s, if from.is_some() { fade } else { 1.0 }));
        let glow = self.anim(Anim::Glow, 1.0);
        let width = self.anim(Anim::PillWidth, pill_width(&s));
        container(lines)
            .padding(Padding {
                top: 9.0,
                bottom: 9.0,
                left: 14.0,
                right: 0.0,
            })
            .width(Length::Fixed(width.max(40.0)))
            .clip(true)
            .style(style::status_island(
                from.map_or(s.tone, |p| p.tone),
                s.tone,
                if from.is_some() { fade } else { 1.0 },
                glow,
            ))
            .into()
    }

    /// Shown while another Apple device has the audio: the AirPods are still connected
    /// here, and one press brings the sound back to this PC.
    pub(super) fn elsewhere_banner<'a>(&self, d: &DeviceSnapshot) -> Element<'a, Message> {
        let switching = self
            .taking_audio
            .is_some_and(|t| t.elapsed() < Duration::from_secs(6));
        let label = if switching {
            "Switching…"
        } else {
            "Use on this PC"
        };
        let device = d.elsewhere_name.as_deref().unwrap_or("another device");
        let take = button(text(label).size(13).font(style::INTER_SEMIBOLD))
            .padding(Padding {
                top: 7.0,
                bottom: 7.0,
                left: 14.0,
                right: 14.0,
            })
            .style(style::primary)
            .on_press_maybe((!switching).then_some(Message::TakeAudio));
        group(vec![
            row![
                column![
                    text(format!("Sound is on {device}"))
                        .size(14)
                        .font(style::INTER_MEDIUM),
                    text("Still connected here. Playing something also switches back.")
                        .size(12)
                        .style(dim_text),
                ]
                .spacing(2)
                .width(Fill),
                widgets::pressable(take),
            ]
            .align_y(Center)
            .spacing(12)
            .into(),
        ])
    }

    pub(super) fn controls<'a>(&self, d: &'a DeviceSnapshot) -> Option<Element<'a, Message>> {
        if !d.is_connected() {
            return None;
        }
        let modes = format::listening_modes(d);
        let show_ca = format::shows_conversation_awareness(d);
        if modes.is_empty() && !show_ca {
            return None;
        }
        let enabled = format::controls_enabled(d);

        let mut col = column![].spacing(0);
        if !modes.is_empty() {
            let selected = self.mode_index(d);
            let pos = selected.map(|i| self.anim(Anim::Mode, i as f32));
            col = col
                .push(section_label("Listening mode"))
                .push(mode_tiles(&modes, selected, pos, enabled));
        }
        if show_ca {
            let on = self
                .pending_ca
                .map(|(v, _)| v)
                .or(d.conversation_awareness)
                .unwrap_or(false);
            col = col.push(gap(10.0)).push(group(vec![setting_row(
                "Conversation awareness",
                Some("Lowers media and lets voices in when you speak"),
                switch(
                    on,
                    enabled.then_some(Message::SetConversation as fn(bool) -> Message),
                ),
            )]));
        }
        if !enabled {
            col = col.push(gap(10.0)).push(
                text("Controls return once the connection is back.")
                    .size(12)
                    .style(faint_text)
                    .width(Fill)
                    .align_x(Center),
            );
        }
        Some(col.into())
    }

    // -------------------------------------------------------------- Island
}

/// The animation box on the AirPods tab, in logical px (frames are 2x).
const ART: f32 = 220.0;

/// AirPods 4 get the case-open animation when its frames are installed.
pub(super) fn has_art(d: &DeviceSnapshot) -> bool {
    matches!(
        d.model,
        crate::core::models::ModelKind::AirPods4 | crate::core::models::ModelKind::AirPods4Anc
    ) && crate::ui::anim::dir().is_some()
}
