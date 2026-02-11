//! Confirmation overlay for display changes with countdown timer.

use std::time::{Duration, Instant};

use gartk_core::{Color, InputEvent, Key, MouseButton, Rect, Theme};
use gartk_render::{Renderer, TextAlign, TextStyle};

/// Default timeout for confirmation (15 seconds).
pub const CONFIRM_TIMEOUT_SECS: u64 = 15;

const DIALOG_WIDTH: i32 = 400;
const DIALOG_HEIGHT: i32 = 150;
const BTN_WIDTH: i32 = 120;
const BTN_HEIGHT: u32 = 36;
const BTN_SPACING: i32 = 20;

/// Result of handling an overlay event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmResult {
    /// No action needed.
    None,
    /// User confirmed the changes.
    Confirmed,
    /// User rejected or timeout expired.
    Reverted,
    /// Just redraw (countdown updated).
    Redraw,
}

/// Confirmation overlay that appears after applying display changes.
pub struct ConfirmOverlay {
    /// When the confirmation started.
    start_time: Instant,
    /// Timeout duration.
    timeout: Duration,
    /// Dialog rect (centered in window).
    dialog_rect: Rect,
    /// Keep button rect.
    keep_btn: Rect,
    /// Revert button rect.
    revert_btn: Rect,
    /// Whether keep button is hovered.
    keep_hovered: bool,
    /// Whether revert button is hovered.
    revert_hovered: bool,
}

/// Compute dialog and button rects centered in the given window rect.
fn compute_layout(window_rect: Rect) -> (Rect, Rect, Rect) {
    let dialog_x = window_rect.x + (window_rect.width as i32 - DIALOG_WIDTH) / 2;
    let dialog_y = window_rect.y + (window_rect.height as i32 - DIALOG_HEIGHT) / 2;
    let dialog_rect = Rect::new(dialog_x, dialog_y, DIALOG_WIDTH as u32, DIALOG_HEIGHT as u32);

    let center_x = dialog_x + DIALOG_WIDTH / 2;
    let btn_y = dialog_y + 95;

    let keep_btn = Rect::new(
        center_x - BTN_WIDTH - BTN_SPACING / 2,
        btn_y,
        BTN_WIDTH as u32,
        BTN_HEIGHT,
    );
    let revert_btn = Rect::new(
        center_x + BTN_SPACING / 2,
        btn_y,
        BTN_WIDTH as u32,
        BTN_HEIGHT,
    );

    (dialog_rect, keep_btn, revert_btn)
}

impl ConfirmOverlay {
    /// Create a new confirmation overlay.
    pub fn new(window_rect: Rect) -> Self {
        let (dialog_rect, keep_btn, revert_btn) = compute_layout(window_rect);

        Self {
            start_time: Instant::now(),
            timeout: Duration::from_secs(CONFIRM_TIMEOUT_SECS),
            dialog_rect,
            keep_btn,
            revert_btn,
            keep_hovered: false,
            revert_hovered: false,
        }
    }

    /// Get remaining seconds until timeout.
    pub fn remaining_secs(&self) -> u64 {
        let elapsed = self.start_time.elapsed();
        if elapsed >= self.timeout {
            0
        } else {
            (self.timeout - elapsed).as_secs() + 1
        }
    }

    /// Check if the timeout has expired.
    pub fn is_expired(&self) -> bool {
        self.start_time.elapsed() >= self.timeout
    }

    /// Update the overlay rect (e.g., on window resize).
    pub fn set_rect(&mut self, rect: Rect) {
        let (dialog_rect, keep_btn, revert_btn) = compute_layout(rect);
        self.dialog_rect = dialog_rect;
        self.keep_btn = keep_btn;
        self.revert_btn = revert_btn;
    }

    /// Handle an input event.
    pub fn handle_event(&mut self, event: &InputEvent) -> ConfirmResult {
        // ALWAYS check for timeout first, on EVERY event
        // This is critical for auto-revert to work reliably
        if self.is_expired() {
            tracing::info!(
                "confirm overlay expired after {:?} (timeout was {:?})",
                self.start_time.elapsed(),
                self.timeout
            );
            return ConfirmResult::Reverted;
        }

        match event {
            InputEvent::Key(e) if e.pressed => {
                match e.key {
                    // Enter confirms
                    Key::Return => {
                        tracing::info!("user confirmed display changes via Enter key");
                        ConfirmResult::Confirmed
                    }
                    // Escape reverts
                    Key::Escape => {
                        tracing::info!("user reverted display changes via Escape key");
                        ConfirmResult::Reverted
                    }
                    _ => ConfirmResult::None,
                }
            }
            InputEvent::MouseMove(e) => {
                let old_keep = self.keep_hovered;
                let old_revert = self.revert_hovered;

                self.keep_hovered = self.keep_btn.contains_point(e.position);
                self.revert_hovered = self.revert_btn.contains_point(e.position);

                if self.keep_hovered != old_keep || self.revert_hovered != old_revert {
                    ConfirmResult::Redraw
                } else {
                    ConfirmResult::None
                }
            }
            InputEvent::MousePress(e) if e.button == Some(MouseButton::Left) => {
                if self.keep_btn.contains_point(e.position) {
                    tracing::info!("user confirmed display changes via Keep button");
                    ConfirmResult::Confirmed
                } else if self.revert_btn.contains_point(e.position) {
                    tracing::info!("user reverted display changes via Revert button");
                    ConfirmResult::Reverted
                } else {
                    ConfirmResult::None
                }
            }
            InputEvent::Idle => {
                let remaining = self.remaining_secs();
                if remaining <= 3 {
                    tracing::debug!("confirm overlay: {}s remaining", remaining);
                }
                ConfirmResult::Redraw // Update countdown display
            }
            _ => ConfirmResult::None,
        }
    }

    /// Render the overlay.
    pub fn render(&self, renderer: &Renderer, theme: &Theme) -> anyhow::Result<()> {
        // Dialog background
        renderer.fill_rounded_rect(self.dialog_rect, 12.0, theme.background)?;
        renderer.stroke_rounded_rect(self.dialog_rect, 12.0, theme.border, 2.0)?;

        let dx = self.dialog_rect.x;
        let dy = self.dialog_rect.y;

        // Title
        let title_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size + 4.0)
            .color(theme.foreground)
            .align(TextAlign::Center);

        let title_rect = Rect::new(dx, dy + 20, DIALOG_WIDTH as u32, 30);
        renderer.text_in_rect("Keep these display settings?", title_rect, &title_style)?;

        // Countdown
        let remaining = self.remaining_secs();
        let countdown_text = format!(
            "Reverting in {} second{}...",
            remaining,
            if remaining == 1 { "" } else { "s" }
        );
        let countdown_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(theme.item_description)
            .align(TextAlign::Center);

        let countdown_rect = Rect::new(dx, dy + 55, DIALOG_WIDTH as u32, 24);
        renderer.text_in_rect(&countdown_text, countdown_rect, &countdown_style)?;

        // Keep button
        let keep_bg = if self.keep_hovered {
            theme.selection_background
        } else {
            Color::new(0.2, 0.6, 0.3, 1.0) // Green
        };
        renderer.fill_rounded_rect(self.keep_btn, 6.0, keep_bg)?;

        let btn_text_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(Color::new(1.0, 1.0, 1.0, 1.0))
            .align(TextAlign::Center);
        renderer.text_in_rect("Keep Changes", self.keep_btn, &btn_text_style)?;

        // Revert button
        let revert_bg = if self.revert_hovered {
            theme.item_hover_background
        } else {
            theme.item_background
        };
        renderer.fill_rounded_rect(self.revert_btn, 6.0, revert_bg)?;
        renderer.stroke_rounded_rect(self.revert_btn, 6.0, theme.border, 1.0)?;

        let revert_text_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(theme.foreground)
            .align(TextAlign::Center);
        renderer.text_in_rect("Revert Now", self.revert_btn, &revert_text_style)?;

        Ok(())
    }
}
