use ratatui::style::{Color, Modifier, Style};

pub(crate) const BG: Color = Color::Rgb(8, 12, 16);
pub(crate) const PANEL: Color = Color::Rgb(13, 19, 26);
pub(crate) const BORDER: Color = Color::Rgb(53, 66, 82);
pub(crate) const TEXT: Color = Color::Rgb(218, 226, 234);
pub(crate) const MUTED: Color = Color::Rgb(125, 137, 150);
pub(crate) const CYAN: Color = Color::Rgb(74, 199, 217);
pub(crate) const GREEN: Color = Color::Rgb(119, 214, 140);
pub(crate) const AMBER: Color = Color::Rgb(228, 180, 84);
pub(crate) const RED: Color = Color::Rgb(238, 106, 106);
pub(crate) const BLUE: Color = Color::Rgb(122, 162, 247);
pub(crate) const CYAN_BG: Color = Color::Rgb(9, 39, 47);
pub(crate) const GREEN_BG: Color = Color::Rgb(12, 42, 27);
pub(crate) const RED_BG: Color = Color::Rgb(49, 22, 26);
pub(crate) const BLUE_BG: Color = Color::Rgb(16, 30, 58);
pub(crate) const AMBER_BG: Color = Color::Rgb(46, 36, 18);

pub(crate) fn title() -> Style {
    Style::default()
        .fg(CYAN)
        .bg(BG)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn border() -> Style {
    Style::default().fg(BORDER).bg(PANEL)
}

pub(crate) fn selected() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(CYAN)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn muted() -> Style {
    Style::default().fg(MUTED).bg(PANEL)
}
