// ABOUTME: Agent picker widget with starfield background.
// ABOUTME: Fuzzy search and keyboard navigation.

#![allow(dead_code)]

use fold_client::Agent;
use rand::Rng;
use ratatui::prelude::*;
use std::collections::HashMap;

pub struct PickerWidget {
    agents: Vec<Agent>,
    filtered_indices: Vec<usize>,
    selected_idx: usize,
    search_query: String,
    starfield: Starfield,
    unread_counts: HashMap<String, u32>,
}

struct Starfield {
    stars: Vec<Star>,
    frame: u64,
    width: u16,
    height: u16,
}

struct Star {
    x: f32,
    y: f32,
    depth: u8,
    char: char,
}

impl PickerWidget {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            filtered_indices: Vec::new(),
            selected_idx: 0,
            search_query: String::new(),
            starfield: Starfield::new(80, 24),
            unread_counts: HashMap::new(),
        }
    }

    pub fn set_agents(&mut self, agents: &[Agent]) {
        self.agents = agents.to_vec();
        self.update_filter();
    }

    pub fn selected_agent(&self) -> Option<&Agent> {
        self.filtered_indices
            .get(self.selected_idx)
            .and_then(|&idx| self.agents.get(idx))
    }

    pub fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected_idx = (self.selected_idx + 1) % self.filtered_indices.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected_idx = self
                .selected_idx
                .checked_sub(1)
                .unwrap_or(self.filtered_indices.len().saturating_sub(1));
        }
    }

    pub fn input_char(&mut self, c: char) {
        self.search_query.push(c);
        self.update_filter();
    }

    pub fn delete_char(&mut self) {
        self.search_query.pop();
        self.update_filter();
    }

    pub fn tick(&mut self) {
        self.starfield.tick();
    }

    /// Resize the starfield to match the given dimensions.
    /// Regenerates stars if dimensions change significantly.
    pub fn resize_starfield(&mut self, width: u16, height: u16) {
        self.starfield.resize(width, height);
    }

    /// Update the unread counts for all agents.
    pub fn set_unread_counts(&mut self, counts: &HashMap<String, u32>) {
        self.unread_counts = counts.clone();
    }

    fn update_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        self.filtered_indices = self
            .agents
            .iter()
            .enumerate()
            .filter(|(_, agent)| query.is_empty() || agent.name.to_lowercase().contains(&query))
            .map(|(idx, _)| idx)
            .collect();
        self.selected_idx = 0;
    }
}

impl Starfield {
    fn new(width: u16, height: u16) -> Self {
        let mut starfield = Self {
            stars: Vec::new(),
            frame: 0,
            width,
            height,
        };
        starfield.generate_stars(width, height);
        starfield
    }

    fn generate_stars(&mut self, width: u16, height: u16) {
        let mut rng = rand::thread_rng();
        let num_stars = (width as usize * height as usize) / 20;
        let chars = ['·', '∙', '•', '⋆', '✦'];

        // Pre-allocate vector with exact capacity to avoid reallocations
        self.stars = Vec::with_capacity(num_stars);
        for _ in 0..num_stars {
            self.stars.push(Star {
                x: rng.gen_range(0.0..width as f32),
                y: rng.gen_range(0.0..height as f32),
                depth: rng.gen_range(0..3),
                char: chars[rng.gen_range(0..chars.len())],
            });
        }
        self.width = width;
        self.height = height;
    }

    fn resize(&mut self, width: u16, height: u16) {
        // Regenerate stars if dimensions change significantly (>20% difference)
        let width_diff = (width as i32 - self.width as i32).unsigned_abs();
        let height_diff = (height as i32 - self.height as i32).unsigned_abs();
        let threshold_w = self.width / 5;
        let threshold_h = self.height / 5;

        if width_diff > threshold_w as u32 || height_diff > threshold_h as u32 {
            self.generate_stars(width, height);
        }
    }

    fn tick(&mut self) {
        self.frame += 1;
        let width = self.width as f32;
        let speeds = [-0.2, -0.5, -1.0];
        for star in &mut self.stars {
            star.x += speeds[star.depth as usize];
            if star.x < 0.0 {
                star.x += width;
            }
        }
    }
}

impl Widget for &PickerWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Dark overlay background using set_style for efficiency
        buf.set_style(area, Style::default().bg(Color::Rgb(10, 10, 20)));

        // Render stars
        for star in &self.starfield.stars {
            let x = area.x + (star.x as u16 % area.width);
            let y = area.y + (star.y as u16 % area.height);
            if x < area.x + area.width && y < area.y + area.height {
                let brightness = match star.depth {
                    0 => Color::Rgb(60, 60, 80),
                    1 => Color::Rgb(100, 100, 130),
                    _ => Color::Rgb(150, 150, 180),
                };
                buf[(x, y)].set_char(star.char).set_fg(brightness);
            }
        }

        // Center box for agent list
        let box_width = 50.min(area.width.saturating_sub(4));
        let box_height = 15.min(area.height.saturating_sub(4));
        let box_x = area.x + (area.width - box_width) / 2;
        let box_y = area.y + (area.height - box_height) / 2;
        let box_area = Rect::new(box_x, box_y, box_width, box_height);

        // Box background using set_style for efficiency
        buf.set_style(box_area, Style::default().bg(Color::Rgb(25, 25, 35)));

        // Title
        let title = " Select Agent ";
        let title_x = box_x + (box_width.saturating_sub(title.len() as u16)) / 2;
        buf.set_string(
            title_x,
            box_y,
            title,
            Style::default().fg(Color::Rgb(180, 180, 200)),
        );

        // Search query
        let search_y = box_y + 2;
        let search_text = format!(" > {} ", self.search_query);
        buf.set_string(
            box_x + 2,
            search_y,
            &search_text,
            Style::default().fg(Color::Rgb(100, 140, 255)),
        );

        // Agent list
        let list_start_y = search_y + 2;
        for (i, &idx) in self.filtered_indices.iter().enumerate() {
            if i >= (box_height.saturating_sub(5)) as usize {
                break;
            }
            let agent = &self.agents[idx];
            let y = list_start_y + i as u16;
            let is_selected = i == self.selected_idx;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Rgb(255, 255, 255))
                    .bg(Color::Rgb(60, 60, 100))
            } else {
                Style::default().fg(Color::Rgb(180, 180, 200))
            };
            let status = if agent.connected { "●" } else { "○" };
            let line = format!(" {} {} ", status, agent.name);
            buf.set_string(box_x + 2, y, &line, style);

            // Display unread badge if count > 0
            if let Some(&count) = self.unread_counts.get(&agent.id) {
                if count > 0 {
                    let badge = format!(" ●{}", count);
                    let badge_x = box_x + 2 + line.len() as u16;
                    let badge_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(255, 180, 100))
                            .bg(Color::Rgb(60, 60, 100))
                    } else {
                        Style::default().fg(Color::Rgb(255, 180, 100))
                    };
                    buf.set_string(badge_x, y, &badge, badge_style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_agent(name: &str, connected: bool) -> Agent {
        Agent {
            id: format!("{}-id", name),
            name: name.to_string(),
            backend: "test-backend".to_string(),
            working_dir: "/tmp/test".to_string(),
            connected,
        }
    }

    #[test]
    fn test_picker_widget_new() {
        let widget = PickerWidget::new();
        assert!(widget.agents.is_empty());
        assert!(widget.filtered_indices.is_empty());
        assert_eq!(widget.selected_idx, 0);
        assert!(widget.search_query.is_empty());
    }

    #[test]
    fn test_set_agents() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
            make_test_agent("Gamma", true),
        ];

        widget.set_agents(&agents);

        assert_eq!(widget.agents.len(), 3);
        assert_eq!(widget.filtered_indices.len(), 3);
        assert_eq!(widget.filtered_indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_update_filter_empty_query() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
        ];
        widget.set_agents(&agents);

        // Empty query should show all agents
        assert_eq!(widget.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_update_filter_with_query() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
            make_test_agent("AlphaTwo", true),
        ];
        widget.set_agents(&agents);

        widget.input_char('a');
        widget.input_char('l');
        widget.input_char('p');
        widget.input_char('h');
        widget.input_char('a');

        // Should filter to agents containing "alpha" (case insensitive)
        assert_eq!(widget.filtered_indices, vec![0, 2]);
        assert_eq!(widget.selected_idx, 0);
    }

    #[test]
    fn test_update_filter_case_insensitive() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("UPPERCASE", true),
            make_test_agent("lowercase", false),
            make_test_agent("MixedCase", true),
        ];
        widget.set_agents(&agents);

        // Typing lowercase should still match uppercase agent names
        widget.input_char('c');
        widget.input_char('a');
        widget.input_char('s');
        widget.input_char('e');

        // "case" matches "UPPERCASE", "lowercase", and "MixedCase"
        assert_eq!(widget.filtered_indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_update_filter_no_matches() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
        ];
        widget.set_agents(&agents);

        widget.input_char('x');
        widget.input_char('y');
        widget.input_char('z');

        // No agent contains "xyz"
        assert!(widget.filtered_indices.is_empty());
    }

    #[test]
    fn test_delete_char() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
        ];
        widget.set_agents(&agents);

        widget.input_char('a');
        widget.input_char('l');
        assert_eq!(widget.search_query, "al");
        assert_eq!(widget.filtered_indices, vec![0]); // Only Alpha

        widget.delete_char();
        assert_eq!(widget.search_query, "a");
        assert_eq!(widget.filtered_indices, vec![0, 1]); // Alpha and Beta

        widget.delete_char();
        assert_eq!(widget.search_query, "");
        assert_eq!(widget.filtered_indices, vec![0, 1]); // All agents
    }

    #[test]
    fn test_select_next_wraps_around() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
            make_test_agent("Gamma", true),
        ];
        widget.set_agents(&agents);

        assert_eq!(widget.selected_idx, 0);

        widget.select_next();
        assert_eq!(widget.selected_idx, 1);

        widget.select_next();
        assert_eq!(widget.selected_idx, 2);

        // Should wrap around to 0
        widget.select_next();
        assert_eq!(widget.selected_idx, 0);
    }

    #[test]
    fn test_select_previous_wraps_around() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
            make_test_agent("Gamma", true),
        ];
        widget.set_agents(&agents);

        assert_eq!(widget.selected_idx, 0);

        // Should wrap around to last item
        widget.select_previous();
        assert_eq!(widget.selected_idx, 2);

        widget.select_previous();
        assert_eq!(widget.selected_idx, 1);

        widget.select_previous();
        assert_eq!(widget.selected_idx, 0);
    }

    #[test]
    fn test_select_next_empty_list() {
        let mut widget = PickerWidget::new();
        // No agents set

        // Should not panic with empty list
        widget.select_next();
        assert_eq!(widget.selected_idx, 0);
    }

    #[test]
    fn test_select_previous_empty_list() {
        let mut widget = PickerWidget::new();
        // No agents set

        // Should not panic with empty list
        widget.select_previous();
        assert_eq!(widget.selected_idx, 0);
    }

    #[test]
    fn test_selected_agent() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
        ];
        widget.set_agents(&agents);

        let selected = widget.selected_agent();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().name, "Alpha");

        widget.select_next();
        let selected = widget.selected_agent();
        assert_eq!(selected.unwrap().name, "Beta");
    }

    #[test]
    fn test_selected_agent_empty_list() {
        let widget = PickerWidget::new();
        assert!(widget.selected_agent().is_none());
    }

    #[test]
    fn test_starfield_generate_stars_count() {
        let starfield = Starfield::new(80, 24);

        // num_stars = (80 * 24) / 20 = 96
        let expected_stars = (80 * 24) / 20;
        assert_eq!(starfield.stars.len(), expected_stars);
    }

    #[test]
    fn test_starfield_generate_stars_bounds() {
        let width = 100u16;
        let height = 50u16;
        let starfield = Starfield::new(width, height);

        // All stars should be within bounds
        for star in &starfield.stars {
            assert!(star.x >= 0.0 && star.x < width as f32);
            assert!(star.y >= 0.0 && star.y < height as f32);
            assert!(star.depth < 3);
        }
    }

    #[test]
    fn test_starfield_tick_moves_stars() {
        let mut starfield = Starfield::new(80, 24);

        // Store initial x positions
        let initial_positions: Vec<f32> = starfield.stars.iter().map(|s| s.x).collect();

        starfield.tick();

        // At least some stars should have moved (all stars move left)
        let moved = starfield
            .stars
            .iter()
            .zip(initial_positions.iter())
            .any(|(star, &initial_x)| (star.x - initial_x).abs() > 0.001);

        assert!(moved, "Stars should move after tick");
    }

    #[test]
    fn test_starfield_resize_regenerates_on_significant_change() {
        let mut starfield = Starfield::new(80, 24);
        let initial_star_count = starfield.stars.len();

        // Resize with significant change (>20%)
        starfield.resize(120, 36);

        // Stars should be regenerated with new count
        let expected_new_count = (120 * 36) / 20;
        assert_eq!(starfield.stars.len(), expected_new_count);
        assert_ne!(starfield.stars.len(), initial_star_count);
        assert_eq!(starfield.width, 120);
        assert_eq!(starfield.height, 36);
    }

    #[test]
    fn test_starfield_resize_no_regenerate_on_small_change() {
        let mut starfield = Starfield::new(80, 24);

        // Store star count
        let initial_star_count = starfield.stars.len();

        // Resize with small change (<20%)
        starfield.resize(82, 25);

        // Stars should NOT be regenerated
        assert_eq!(starfield.stars.len(), initial_star_count);
        // Dimensions should remain unchanged
        assert_eq!(starfield.width, 80);
        assert_eq!(starfield.height, 24);
    }

    #[test]
    fn test_picker_tick_advances_starfield() {
        let mut widget = PickerWidget::new();
        let initial_frame = widget.starfield.frame;

        widget.tick();

        assert_eq!(widget.starfield.frame, initial_frame + 1);
    }

    #[test]
    fn test_resize_starfield() {
        let mut widget = PickerWidget::new();
        assert_eq!(widget.starfield.width, 80);
        assert_eq!(widget.starfield.height, 24);

        widget.resize_starfield(160, 48);

        assert_eq!(widget.starfield.width, 160);
        assert_eq!(widget.starfield.height, 48);
    }

    #[test]
    fn test_set_unread_counts() {
        let mut widget = PickerWidget::new();
        let agents = vec![
            make_test_agent("Alpha", true),
            make_test_agent("Beta", false),
        ];
        widget.set_agents(&agents);

        // Initially no unread counts
        assert!(widget.unread_counts.is_empty());

        // Set some unread counts
        let mut counts = HashMap::new();
        counts.insert("Alpha-id".to_string(), 3);
        counts.insert("Beta-id".to_string(), 1);
        widget.set_unread_counts(&counts);

        assert_eq!(widget.unread_counts.get("Alpha-id"), Some(&3));
        assert_eq!(widget.unread_counts.get("Beta-id"), Some(&1));
    }

    #[test]
    fn test_set_unread_counts_clears_when_empty() {
        let mut widget = PickerWidget::new();
        let agents = vec![make_test_agent("Alpha", true)];
        widget.set_agents(&agents);

        // Set some unread counts
        let mut counts = HashMap::new();
        counts.insert("Alpha-id".to_string(), 5);
        widget.set_unread_counts(&counts);
        assert_eq!(widget.unread_counts.get("Alpha-id"), Some(&5));

        // Clear unread counts
        widget.set_unread_counts(&HashMap::new());
        assert!(widget.unread_counts.is_empty());
    }

    #[test]
    fn test_unread_counts_initialized_empty() {
        let widget = PickerWidget::new();
        assert!(widget.unread_counts.is_empty());
    }
}
