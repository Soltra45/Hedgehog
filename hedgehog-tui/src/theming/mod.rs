mod selectors;
mod style_parser;

use crate::cmdreader::{self, CommandReader, FileResolver};
use cmd_parser::CmdParsable;
use selectors::StyleSelector;
pub(crate) use selectors::{
    Empty, List, ListColumn, ListItem, ListState, Player, Selector, StatusBar,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tui::style::Style;

#[derive(Debug, Clone, Copy)]
struct OverridableStyle {
    style: Style,
    inherited: bool,
}

impl Default for OverridableStyle {
    fn default() -> Self {
        OverridableStyle {
            style: Style::default(),
            inherited: false,
        }
    }
}

pub(crate) struct Theme {
    styles: HashMap<Selector, Style>,
}

impl Theme {
    pub(crate) fn handle_command(&mut self, command: ThemeCommand) -> Result<(), cmdreader::Error> {
        match command {
            ThemeCommand::Reset => *self = Theme::default(),
            ThemeCommand::Set(selector, style) => self.set(selector, style),
            ThemeCommand::Load(path, loading_option) => {
                if let ThemeLoadingMode::Reset = loading_option.unwrap_or_default() {
                    *self = Theme::default();
                }
                let resolver = FileResolver::new()
                    .with_suffix(".theme")
                    .with_reversed_order();
                let path = resolver.resolve(path).ok_or(cmdreader::Error::Resolution)?;

                let mut reader = CommandReader::open(path)?;
                while let Some(command) = reader.read()? {
                    self.handle_command(command)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn get(&self, selector: impl Into<Selector>) -> Style {
        self.styles
            .get(&selector.into())
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn set(&mut self, selector: impl Into<Selector>, style: Style) {
        let styles = &mut self.styles;
        let mut override_style = move |selector: Selector| {
            styles
                .entry(selector)
                .and_modify(|current| *current = current.patch(style))
                .or_insert(style);
        };

        let selector = selector.into();
        override_style(selector);
        selector.for_each_overrides(override_style);
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            styles: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, CmdParsable)]
pub(crate) enum ThemeLoadingMode {
    Reset,
    NoReset,
}

impl Default for ThemeLoadingMode {
    fn default() -> Self {
        ThemeLoadingMode::Reset
    }
}

#[derive(Debug, Clone, PartialEq, CmdParsable)]
pub(crate) enum ThemeCommand {
    Reset,
    Set(
        Selector,
        #[cmd(parse_with = "style_parser::parse_cmd")] Style,
    ),
    Load(PathBuf, Option<ThemeLoadingMode>),
}

#[cfg(test)]
mod tests {
    use super::{List, ListItem, ListState, StatusBar, Theme};
    use crate::status::Severity;
    use tui::style::{Color, Modifier, Style};

    #[test]
    fn status_bar_styles() {
        let mut theme = Theme::default();
        assert_eq!(theme.get(StatusBar::Empty), Style::default());
        theme.set(StatusBar::Empty, Style::default().fg(Color::Red));
        assert_eq!(theme.get(StatusBar::Empty), Style::default().fg(Color::Red));
    }

    #[test]
    fn status_bar_status() {
        let mut theme = Theme::default();
        theme.set(StatusBar::Status(None), Style::default().bg(Color::Red));
        theme.set(
            StatusBar::Status(Some(Severity::Error)),
            Style::default().fg(Color::White),
        );
        assert_eq!(
            theme.get(StatusBar::Status(Some(Severity::Error))),
            Style::default().bg(Color::Red).fg(Color::White)
        );
        assert_eq!(
            theme.get(StatusBar::Status(Some(Severity::Information))),
            Style::default().bg(Color::Red)
        );
    }

    #[test]
    fn divider_styles() {
        let mut theme = Theme::default();
        assert_eq!(theme.get(List::Divider), Style::default());
        theme.set(List::Divider, Style::default().fg(Color::Red));
        assert_eq!(theme.get(List::Divider), Style::default().fg(Color::Red));
    }

    #[test]
    fn list_item_style() {
        let mut theme = Theme::default();
        theme.set(
            List::Item(ListItem::default()),
            Style::default().fg(Color::White),
        );
        theme.set(
            List::Item(ListItem {
                missing_title: true,
                ..Default::default()
            }),
            Style::default().add_modifier(Modifier::RAPID_BLINK),
        );
        theme.set(
            List::Item(ListItem {
                selected: true,
                ..Default::default()
            }),
            Style::default().bg(Color::Red),
        );
        theme.set(
            List::Item(ListItem {
                focused: true,
                ..Default::default()
            }),
            Style::default().add_modifier(Modifier::BOLD),
        );
        theme.set(
            List::Item(ListItem {
                state: Some(ListState::EpisodePlaying),
                ..Default::default()
            }),
            Style::default().add_modifier(Modifier::UNDERLINED),
        );

        let selected_focused = theme.get(List::Item(ListItem {
            selected: true,
            focused: true,
            ..Default::default()
        }));
        assert_eq!(
            selected_focused,
            Style {
                fg: Some(Color::White),
                bg: Some(Color::Red),
                add_modifier: Modifier::BOLD,
                sub_modifier: Modifier::empty(),
            }
        );

        let focused_playing = theme.get(List::Item(ListItem {
            focused: true,
            state: Some(ListState::EpisodePlaying),
            ..Default::default()
        }));
        assert_eq!(
            focused_playing,
            Style {
                fg: Some(Color::White),
                bg: None,
                add_modifier: Modifier::BOLD | Modifier::UNDERLINED,
                sub_modifier: Modifier::empty(),
            }
        );

        let focused_active_missing = theme.get(List::Item(ListItem {
            focused: true,
            state: Some(ListState::EpisodePlaying),
            missing_title: true,
            ..Default::default()
        }));
        assert_eq!(
            focused_active_missing,
            Style {
                fg: Some(Color::White),
                bg: None,
                add_modifier: Modifier::BOLD | Modifier::UNDERLINED | Modifier::RAPID_BLINK,
                sub_modifier: Modifier::empty(),
            }
        );
    }
}
