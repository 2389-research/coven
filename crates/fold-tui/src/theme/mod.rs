// ABOUTME: Theme system with semantic color roles.
// ABOUTME: Built-in themes and theme registry.

#![allow(dead_code)]

use ratatui::style::Color;

pub struct Theme {
    pub name: &'static str,
    pub background: Color,
    pub surface: Color,
    pub surface_dim: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_inverse: Color,
    pub primary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub user_message: Color,
    pub agent_message: Color,
    pub thinking: Color,
    pub tool_use: Color,
}

pub static DEFAULT_THEME: Theme = Theme {
    name: "default",
    background: Color::Rgb(22, 22, 30),
    surface: Color::Rgb(32, 32, 44),
    surface_dim: Color::Rgb(26, 26, 36),
    text: Color::Rgb(230, 230, 240),
    text_muted: Color::Rgb(140, 140, 160),
    text_inverse: Color::Rgb(22, 22, 30),
    primary: Color::Rgb(100, 140, 255),
    accent: Color::Rgb(140, 100, 255),
    success: Color::Rgb(100, 220, 140),
    warning: Color::Rgb(255, 200, 100),
    error: Color::Rgb(255, 100, 120),
    user_message: Color::Rgb(100, 180, 255),
    agent_message: Color::Rgb(180, 140, 255),
    thinking: Color::Rgb(255, 200, 140),
    tool_use: Color::Rgb(140, 200, 180),
};

pub static LIGHT_THEME: Theme = Theme {
    name: "light",
    background: Color::Rgb(250, 250, 252),
    surface: Color::Rgb(255, 255, 255),
    surface_dim: Color::Rgb(240, 240, 245),
    text: Color::Rgb(30, 30, 40),
    text_muted: Color::Rgb(100, 100, 120),
    text_inverse: Color::Rgb(250, 250, 252),
    primary: Color::Rgb(50, 100, 220),
    accent: Color::Rgb(100, 60, 200),
    success: Color::Rgb(40, 160, 80),
    warning: Color::Rgb(200, 140, 20),
    error: Color::Rgb(200, 50, 70),
    user_message: Color::Rgb(40, 120, 200),
    agent_message: Color::Rgb(120, 80, 180),
    thinking: Color::Rgb(180, 130, 40),
    tool_use: Color::Rgb(60, 140, 120),
};

pub static MIDNIGHT_THEME: Theme = Theme {
    name: "midnight",
    background: Color::Rgb(12, 14, 28),
    surface: Color::Rgb(20, 24, 45),
    surface_dim: Color::Rgb(16, 18, 36),
    text: Color::Rgb(200, 210, 235),
    text_muted: Color::Rgb(100, 115, 150),
    text_inverse: Color::Rgb(12, 14, 28),
    primary: Color::Rgb(80, 140, 240),
    accent: Color::Rgb(100, 160, 255),
    success: Color::Rgb(80, 200, 160),
    warning: Color::Rgb(240, 180, 100),
    error: Color::Rgb(240, 90, 110),
    user_message: Color::Rgb(90, 160, 240),
    agent_message: Color::Rgb(130, 170, 255),
    thinking: Color::Rgb(200, 160, 100),
    tool_use: Color::Rgb(100, 180, 200),
};

pub static EMBER_THEME: Theme = Theme {
    name: "ember",
    background: Color::Rgb(28, 20, 18),
    surface: Color::Rgb(42, 30, 26),
    surface_dim: Color::Rgb(34, 24, 20),
    text: Color::Rgb(240, 225, 210),
    text_muted: Color::Rgb(160, 140, 120),
    text_inverse: Color::Rgb(28, 20, 18),
    primary: Color::Rgb(255, 140, 60),
    accent: Color::Rgb(255, 100, 80),
    success: Color::Rgb(140, 200, 100),
    warning: Color::Rgb(255, 200, 80),
    error: Color::Rgb(255, 80, 80),
    user_message: Color::Rgb(255, 160, 80),
    agent_message: Color::Rgb(255, 120, 100),
    thinking: Color::Rgb(255, 200, 120),
    tool_use: Color::Rgb(200, 180, 120),
};

pub static MATRIX_THEME: Theme = Theme {
    name: "matrix",
    background: Color::Rgb(0, 0, 0),
    surface: Color::Rgb(10, 15, 10),
    surface_dim: Color::Rgb(5, 8, 5),
    text: Color::Rgb(0, 255, 65),
    text_muted: Color::Rgb(0, 140, 40),
    text_inverse: Color::Rgb(0, 0, 0),
    primary: Color::Rgb(0, 255, 100),
    accent: Color::Rgb(100, 255, 100),
    success: Color::Rgb(0, 255, 0),
    warning: Color::Rgb(200, 255, 0),
    error: Color::Rgb(255, 60, 60),
    user_message: Color::Rgb(80, 255, 120),
    agent_message: Color::Rgb(0, 220, 80),
    thinking: Color::Rgb(180, 255, 80),
    tool_use: Color::Rgb(0, 200, 150),
};

pub static ROSE_THEME: Theme = Theme {
    name: "rose",
    background: Color::Rgb(28, 18, 32),
    surface: Color::Rgb(42, 28, 48),
    surface_dim: Color::Rgb(34, 22, 38),
    text: Color::Rgb(240, 225, 240),
    text_muted: Color::Rgb(160, 130, 165),
    text_inverse: Color::Rgb(28, 18, 32),
    primary: Color::Rgb(255, 120, 180),
    accent: Color::Rgb(200, 100, 255),
    success: Color::Rgb(140, 220, 160),
    warning: Color::Rgb(255, 200, 140),
    error: Color::Rgb(255, 100, 120),
    user_message: Color::Rgb(255, 140, 190),
    agent_message: Color::Rgb(180, 120, 255),
    thinking: Color::Rgb(255, 180, 200),
    tool_use: Color::Rgb(180, 160, 220),
};

pub fn get_theme(name: &str) -> &'static Theme {
    match name {
        "default" => &DEFAULT_THEME,
        "light" => &LIGHT_THEME,
        "midnight" => &MIDNIGHT_THEME,
        "ember" => &EMBER_THEME,
        "matrix" => &MATRIX_THEME,
        "rose" => &ROSE_THEME,
        _ => &DEFAULT_THEME,
    }
}

pub fn list_themes() -> &'static [&'static str] {
    &["default", "light", "midnight", "ember", "matrix", "rose"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;

    /// Helper to serialize a Theme to a stable text format for snapshots.
    fn theme_to_snapshot(theme: &Theme) -> String {
        let color_to_str = |c: Color| -> String {
            match c {
                Color::Rgb(r, g, b) => format!("rgb({}, {}, {})", r, g, b),
                other => format!("{:?}", other),
            }
        };

        format!(
            r#"Theme: {}
  background: {}
  surface: {}
  surface_dim: {}
  text: {}
  text_muted: {}
  text_inverse: {}
  primary: {}
  accent: {}
  success: {}
  warning: {}
  error: {}
  user_message: {}
  agent_message: {}
  thinking: {}
  tool_use: {}"#,
            theme.name,
            color_to_str(theme.background),
            color_to_str(theme.surface),
            color_to_str(theme.surface_dim),
            color_to_str(theme.text),
            color_to_str(theme.text_muted),
            color_to_str(theme.text_inverse),
            color_to_str(theme.primary),
            color_to_str(theme.accent),
            color_to_str(theme.success),
            color_to_str(theme.warning),
            color_to_str(theme.error),
            color_to_str(theme.user_message),
            color_to_str(theme.agent_message),
            color_to_str(theme.thinking),
            color_to_str(theme.tool_use),
        )
    }

    #[test]
    fn test_default_theme_snapshot() {
        assert_snapshot!(theme_to_snapshot(&DEFAULT_THEME));
    }

    #[test]
    fn test_light_theme_snapshot() {
        assert_snapshot!(theme_to_snapshot(&LIGHT_THEME));
    }

    #[test]
    fn test_midnight_theme_snapshot() {
        assert_snapshot!(theme_to_snapshot(&MIDNIGHT_THEME));
    }

    #[test]
    fn test_ember_theme_snapshot() {
        assert_snapshot!(theme_to_snapshot(&EMBER_THEME));
    }

    #[test]
    fn test_matrix_theme_snapshot() {
        assert_snapshot!(theme_to_snapshot(&MATRIX_THEME));
    }

    #[test]
    fn test_rose_theme_snapshot() {
        assert_snapshot!(theme_to_snapshot(&ROSE_THEME));
    }

    #[test]
    fn test_theme_list_snapshot() {
        let themes = list_themes();
        assert_snapshot!(themes.join("\n"));
    }

    #[test]
    fn test_get_theme_returns_correct_theme() {
        assert_eq!(get_theme("default").name, "default");
        assert_eq!(get_theme("light").name, "light");
        assert_eq!(get_theme("midnight").name, "midnight");
        assert_eq!(get_theme("ember").name, "ember");
        assert_eq!(get_theme("matrix").name, "matrix");
        assert_eq!(get_theme("rose").name, "rose");
    }

    #[test]
    fn test_get_theme_unknown_returns_default() {
        let theme = get_theme("unknown");
        assert_eq!(theme.name, "default");
    }

    #[test]
    fn test_list_themes_returns_all_themes() {
        let themes = list_themes();
        assert_eq!(themes.len(), 6);
        assert!(themes.contains(&"default"));
        assert!(themes.contains(&"light"));
        assert!(themes.contains(&"midnight"));
        assert!(themes.contains(&"ember"));
        assert!(themes.contains(&"matrix"));
        assert!(themes.contains(&"rose"));
    }

    #[test]
    fn test_light_theme_has_light_background() {
        let theme = get_theme("light");
        if let Color::Rgb(r, g, b) = theme.background {
            assert!(r > 200 && g > 200 && b > 200);
        } else {
            panic!("Expected RGB color");
        }
    }

    #[test]
    fn test_matrix_theme_has_black_background() {
        let theme = get_theme("matrix");
        assert_eq!(theme.background, Color::Rgb(0, 0, 0));
    }
}
