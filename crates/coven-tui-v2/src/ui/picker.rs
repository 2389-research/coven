// ABOUTME: Agent picker overlay rendering
// ABOUTME: Centered modal with filterable agent list

use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App) {
    // Center overlay: 60% width, 50% height
    let area = centered_rect(60, 50, f.area());

    // Clear background
    f.render_widget(Clear, area);

    // Filter agents
    let filtered = app.filtered_agents();

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let icon = if agent.connected { "●" } else { "○" };
            let model = agent.model.as_deref().unwrap_or(&agent.backend);
            let text = format!(" {} {} ({})", icon, agent.name, model);

            let style = if i == app.picker_index {
                Style::default().reversed()
            } else if !agent.connected {
                Style::default().dim()
            } else {
                Style::default()
            };

            ListItem::new(text).style(style)
        })
        .collect();

    let title = if app.picker_filter.is_empty() {
        " Select Agent (type to filter) ".to_string()
    } else {
        format!(" Filter: {} ", app.picker_filter)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().cyan())
            .title(title),
    );

    f.render_widget(list, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
