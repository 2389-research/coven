// ABOUTME: Themes command implementation.
// ABOUTME: Lists available themes and sets the active theme.

use crate::error::{AppError, Result};
use crate::state::config::Config;
use crate::theme;

/// List all available themes, marking the active one.
pub fn list_themes(config: &Config) {
    let available = theme::list_themes();
    let current = &config.appearance.theme;

    println!("Available themes:");
    for name in available {
        if *name == current {
            println!("  \u{25cf} {} (active)", name);
        } else {
            println!("    {}", name);
        }
    }
}

/// Set the active theme in config and save it.
pub fn set_theme(config: &mut Config, name: &str) -> Result<()> {
    let available = theme::list_themes();

    if !available.contains(&name) {
        let available_list = available
            .iter()
            .map(|t| format!("  {}", t))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(AppError::UnknownTheme {
            name: name.to_string(),
            available: available_list,
        });
    }

    config.appearance.theme = name.to_string();
    config.save()?;

    println!("Theme set to: {}", name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_theme_validates_name() {
        let available = theme::list_themes();
        assert!(available.contains(&"default"));
        assert!(available.contains(&"midnight"));
        assert!(!available.contains(&"nonexistent"));
    }

    #[test]
    fn test_list_themes_matches_theme_module() {
        let available = theme::list_themes();
        assert_eq!(available.len(), 6);
        assert!(available.contains(&"default"));
        assert!(available.contains(&"light"));
        assert!(available.contains(&"midnight"));
        assert!(available.contains(&"ember"));
        assert!(available.contains(&"matrix"));
        assert!(available.contains(&"rose"));
    }
}
