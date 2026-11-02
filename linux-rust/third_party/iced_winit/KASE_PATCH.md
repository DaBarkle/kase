# Kase patch

Upstream `iced_winit` 0.14.0 with one change: `window::Action::Resize` applies the size that
`winit::Window::request_inner_size` returns. On Wayland winit applies the size at once and
sends no `Resized` event, so upstream left the viewport and GPU surface at the old size and
KWin stretched the old frame into the resized window (the morphing window looked squashed).
See `State::resized` in `src/window/state.rs`.
