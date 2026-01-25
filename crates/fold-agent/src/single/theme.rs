// ABOUTME: Neo-terminal color theme for single mode TUI
// ABOUTME: Inspired by hex's aesthetic - deep ink, coral, sage, sky

#![allow(dead_code)]

use ratatui::style::Color;

// Primary palette
pub const DEEP_INK: Color = Color::Rgb(26, 27, 38);
pub const SOFT_PAPER: Color = Color::Rgb(192, 202, 245);
pub const ACCENT_CORAL: Color = Color::Rgb(255, 158, 100);
pub const ACCENT_SAGE: Color = Color::Rgb(158, 206, 106);
pub const ACCENT_SKY: Color = Color::Rgb(122, 162, 247);

// Secondary palette
pub const DIM_INK: Color = Color::Rgb(86, 95, 137);
pub const WARNING_AMBER: Color = Color::Rgb(224, 175, 104);
pub const ERROR_RUBY: Color = Color::Rgb(247, 118, 142);
pub const SUCCESS_JADE: Color = Color::Rgb(115, 218, 202);
