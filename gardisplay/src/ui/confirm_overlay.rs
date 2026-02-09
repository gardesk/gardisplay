//! Confirmation overlay for display changes with countdown timer.

use std::time::{Duration, Instant};

use gartk_core::{Color, InputEvent, Key, MouseButton, Rect, Theme};
use gartk_render::{Renderer, TextAlign, TextStyle};

/// Default timeout for confirmation (15 seconds).
pub const CONFIRM_TIMEOUT_SECS: u64 = 15;

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
    /// Full window rect for the overlay.
    rect: Rect,
    /// When the confirmation started.
    start_time: Instant,
    /// Timeout duration.
    timeout: Duration,
    /// Keep button rect.
    keep_btn: Rect,
    /// Revert button rect.
    revert_btn: Rect,
    /// Whether keep button is hovered.
    keep_hovered: bool,
    /// Whether revert button is hovered.
    revert_hovered: bool,
}

impl ConfirmOverlay {
    /// Create a new confirmation overlay.
    pub fn new(window_rect: Rect) -> Self {
        let btn_width = 120;
        let btn_height = 36;
        let btn_spacing = 20;
        let center_x = window_rect.x + window_rect.width as i32 / 2;
        let center_y = window_rect.y + window_rect.height as i32 / 2;

        let keep_btn = Rect::new(
            center_x - btn_width - btn_spacing / 2,
            center_y + 30,
            btn_width as u32,
            btn_height,
        );
        let revert_btn = Rect::new(
            center_x + btn_spacing / 2,
            center_y + 30,
            btn_width as u32,
            btn_height,
        );

        Self {
            rect: window_rect,
            start_time: Instant::now(),
            timeout: Duration::from_secs(CONFIRM_TIMEOUT_SECS),
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
        self.rect = rect;
        let btn_width = 120;
        let btn_height = 36;
        let btn_spacing = 20;
        let center_x = rect.x + rect.width as i32 / 2;
        let center_y = rect.y + rect.height as i32 / 2;

        self.keep_btn = Rect::new(
            center_x - btn_width - btn_spacing / 2,
            center_y + 30,
            btn_width as u32,
            btn_height,
        );
        self.revert_btn = Rect::new(
            center_x + btn_spacing / 2,
            center_y + 30,
            btn_width as u32,
            btn_height,
        );
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
                // Check timeout on idle (already checked above, but log for debugging)
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
        // Semi-transparent dark overlay
        let overlay_color = Color::new(0.0, 0.0, 0.0, 0.7);
        renderer.fill_rect(self.rect, overlay_color)?;

        // Center dialog box
        let dialog_width = 400;
        let dialog_height = 150;
        let dialog_x = self.rect.x + (self.rect.width as i32 - dialog_width) / 2;
        let dialog_y = self.rect.y + (self.rect.height as i32 - dialog_height) / 2;
        let dialog_rect = Rect::new(dialog_x, dialog_y, dialog_width as u32, dialog_height as u32);

        // Dialog background
        renderer.fill_rounded_rect(dialog_rect, 12.0, theme.background)?;
        renderer.stroke_rounded_rect(dialog_rect, 12.0, theme.border, 2.0)?;

        // Title
        let title_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size + 4.0)
            .color(theme.foreground)
            .align(TextAlign::Center);

        let title_rect = Rect::new(dialog_x, dialog_y + 20, dialog_width as u32, 30);
        renderer.text_in_rect("Keep these display settings?", title_rect, &title_style)?;

        // Countdown
        let remaining = self.remaining_secs();
        let countdown_text = format!("Reverting in {} second{}...", remaining, if remaining == 1 { "" } else { "s" });
        let countdown_style = TextStyle::new()
            .font_family(&theme.font_family)
            .font_size(theme.font_size)
            .color(theme.item_description)
            .align(TextAlign::Center);

        let countdown_rect = Rect::new(dialog_x, dialog_y + 55, dialog_width as u32, 24);
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
