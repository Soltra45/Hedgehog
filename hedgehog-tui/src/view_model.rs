use crate::cmdparser;
use crate::cmdreader::{CommandReader, FileResolver};
use crate::dataview::{
    CursorCommand, InteractiveList, ListData, PaginatedData, PaginatedDataMessage, Versioned,
};
use crate::keymap::{Key, KeyMapping};
use crate::screen::{EpisodesListProvider, FeedsListProvider};
use crate::status::{Severity, Status};
use crate::theming::{Theme, ThemeCommand};
use actix::System;
use hedgehog_library::model::{EpisodeSummary, FeedId, FeedSummary};
use hedgehog_library::EpisodeSummariesQuery;
use hedgehog_player::state::PlaybackState;
use hedgehog_player::{volume::VolumeCommand, PlaybackCommand, PlayerNotification};
use serde::Deserialize;
use std::path::PathBuf;

pub(crate) trait PlayerDelegate {
    fn send_volume_command(&self, command: VolumeCommand);
    fn send_playback_command(&self, command: PlaybackCommand);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusedPane {
    FeedsList,
    EpisodesList,
}

pub(crate) struct ViewModel<P> {
    pub(crate) feeds_list: InteractiveList<ListData<FeedSummary>, FeedsListProvider>,
    pub(crate) episodes_list: InteractiveList<PaginatedData<EpisodeSummary>, EpisodesListProvider>,
    pub(crate) status: Option<Status>,
    pub(crate) key_mapping: KeyMapping<Command>,
    pub(crate) theme: Theme,
    pub(crate) focus: FocusedPane,
    selected_feed: Option<FeedId>,
    pub(crate) playback_state: PlaybackState,
    player_delegate: P,
}

impl<P: PlayerDelegate> ViewModel<P> {
    pub(crate) fn new(size: (u16, u16), player_delegate: P) -> Self {
        ViewModel {
            feeds_list: InteractiveList::new(size.1 as usize - 2),
            episodes_list: InteractiveList::new(size.1 as usize - 2),
            status: None,
            key_mapping: KeyMapping::new(),
            theme: Theme::default(),
            focus: FocusedPane::FeedsList,
            selected_feed: None,
            playback_state: PlaybackState::default(),
            player_delegate,
        }
    }

    pub(crate) fn set_size(&mut self, _width: u16, height: u16) {
        self.episodes_list.set_window_size(height as usize - 2);
        self.feeds_list.set_window_size(height as usize - 2);
    }

    pub(crate) fn clear_status(&mut self) {
        self.status = None;
    }

    pub(crate) fn error(&mut self, error: impl std::error::Error) {
        self.status = Some(Status::new_custom(error.to_string(), Severity::Error));
    }

    pub(crate) fn handle_command_str(&mut self, command: &str) {
        match cmdparser::from_str(command) {
            Ok(command) => {
                self.handle_command_interactive(command);
            }
            Err(error) => self.status = Some(Status::CommandParsingError(error)),
        }
    }

    fn handle_command(&mut self, command: Command) -> Result<bool, Status> {
        match command {
            Command::Cursor(command) => {
                match self.focus {
                    FocusedPane::FeedsList => {
                        self.feeds_list.handle_command(command);
                        self.update_current_feed();
                    }
                    FocusedPane::EpisodesList => self.episodes_list.handle_command(command),
                }
                Ok(true)
            }
            Command::ToggleFocus => {
                match self.focus {
                    FocusedPane::FeedsList => self.focus = FocusedPane::EpisodesList,
                    FocusedPane::EpisodesList => self.focus = FocusedPane::FeedsList,
                }
                Ok(true)
            }
            Command::Quit => {
                System::current().stop();
                Ok(false)
            }
            Command::Map(key, command) => {
                let redefined = self.key_mapping.contains(&key);
                self.key_mapping.map(key, *command);

                if redefined {
                    Err(Status::new_custom(
                        "Key mapping redefined",
                        Severity::Information,
                    ))
                } else {
                    Ok(false)
                }
            }
            Command::Unmap(key) => {
                if !self.key_mapping.unmap(&key) {
                    Err(Status::new_custom(
                        "Key mapping is not defined",
                        Severity::Warning,
                    ))
                } else {
                    Ok(false)
                }
            }
            Command::Theme(command) => self
                .theme
                .handle_command(command)
                .map(|_| true)
                .map_err(|error| Status::new_custom(format!("{}", error), Severity::Error)),
            Command::Exec(path) => {
                let mut reader = match CommandReader::open(path) {
                    Ok(reader) => reader,
                    Err(error) => {
                        return Err(Status::new_custom(format!("{}", error), Severity::Error))
                    }
                };

                loop {
                    match reader.read() {
                        Ok(None) => break Ok(true),
                        Ok(Some(command)) => {
                            if let Err(status) = self.handle_command(command) {
                                if status.severity() == Severity::Error {
                                    return Err(status);
                                }
                            }
                        }
                        Err(error) => {
                            return Err(Status::new_custom(format!("{}", error), Severity::Error))
                        }
                    }
                }
            }
            Command::Volume(command) => {
                self.player_delegate.send_volume_command(command);
                Ok(false)
            }
            Command::Playback(command) => {
                self.player_delegate.send_playback_command(command);
                Ok(false)
            }
        }
    }

    pub(crate) fn handle_command_interactive(&mut self, command: Command) -> bool {
        match self.handle_command(command) {
            Ok(should_redraw) => should_redraw,
            Err(status) => {
                self.status = Some(status);
                true
            }
        }
    }

    pub(crate) fn init_rc(&mut self) {
        let resolver = FileResolver::new();
        resolver.visit_all("rc", |path| {
            match self.handle_command(Command::Exec(path.to_path_buf())) {
                Ok(_) => false,
                Err(status) => {
                    if status.severity() == Severity::Error {
                        self.status = Some(status);
                        true
                    } else {
                        false
                    }
                }
            }
        });
    }

    pub(crate) fn set_episodes_list_data(
        &mut self,
        data: Versioned<PaginatedDataMessage<EpisodeSummary>>,
    ) -> bool {
        self.episodes_list.handle_data(data)
    }

    fn update_current_feed(&mut self) {
        let selected_id = self.feeds_list.selection().map(|item| item.id);
        if selected_id == self.selected_feed {
            return;
        }

        if let Some(selected_id) = selected_id {
            self.episodes_list.update_provider(|provider| {
                provider.query = Some(EpisodeSummariesQuery::default().with_feed_id(selected_id));
            });
            self.selected_feed = Some(selected_id);
        }
    }

    pub(crate) fn set_feeds_list_data(&mut self, data: Versioned<Vec<FeedSummary>>) -> bool {
        if self.feeds_list.handle_data(data) {
            self.update_current_feed();
            true
        } else {
            false
        }
    }

    pub(crate) fn handle_player_notification(&mut self, notification: PlayerNotification) {
        match notification {
            PlayerNotification::VolumeChanged(volume) => {
                self.status = Some(Status::VolumeChanged(volume))
            }
            PlayerNotification::StateChanged(state) => self.playback_state.set_state(state),
            PlayerNotification::DurationSet(duration) => self.playback_state.set_duration(duration),
            PlayerNotification::PositionSet(position) => self.playback_state.set_position(position),
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Command {
    #[serde(rename = "line")]
    Cursor(CursorCommand),
    Map(Key, Box<Command>),
    Unmap(Key),
    Theme(ThemeCommand),
    Exec(PathBuf),
    Volume(VolumeCommand),
    Playback(PlaybackCommand),
    #[serde(alias = "q")]
    Quit,
    ToggleFocus,
}

#[cfg(test)]
mod tests {
    use super::{Command, PlayerDelegate, ViewModel};
    use crate::dataview::CursorCommand;
    use crate::theming::{List, StatusBar};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;
    use tui::style::{Color, Style};

    fn write_file(dir: impl AsRef<Path>, filename: impl AsRef<Path>, content: &str) {
        let mut path = dir.as_ref().to_path_buf();
        path.push(filename);

        let mut file = File::create(path).unwrap();
        write!(file, "{}", content).unwrap();
    }

    struct NoopPlayerDelagate;

    impl PlayerDelegate for NoopPlayerDelagate {
        fn send_volume_command(&self, _command: hedgehog_player::volume::VolumeCommand) {}
        fn send_playback_command(&self, _command: hedgehog_player::PlaybackCommand) {}
    }

    #[test]
    fn init_rc() {
        let global_data = tempdir().unwrap();
        let user_data = tempdir().unwrap();
        let env_path = std::env::join_paths([global_data.path(), user_data.path()]).unwrap();
        // TODO: changes global state, environment variable cannot be set by multiple tests
        std::env::set_var("HEDGEHOG_PATH", env_path);

        write_file(
            global_data.path(),
            "rc",
            "map Up line previous\nmap Down line next\ntheme load default",
        );
        write_file(
            global_data.path(),
            "default.theme",
            "load another no-reset\nset statusbar.empty bg:red",
        );
        write_file(
            global_data.path(),
            "another.theme",
            "set list.divider bg:blue",
        );

        write_file(
            user_data.path(),
            "another.theme",
            "set statusbar.command bg:yellow",
        );
        write_file(user_data.path(), "rc", "map Down line last");

        let mut view_model = ViewModel::new((32, 32), NoopPlayerDelagate);
        view_model.init_rc();

        assert!(view_model.status.is_none());
        assert_eq!(
            view_model
                .key_mapping
                .get(&KeyEvent::new(KeyCode::Up, KeyModifiers::empty()).into())
                .unwrap(),
            &Command::Cursor(CursorCommand::Previous)
        );
        assert_eq!(
            view_model
                .key_mapping
                .get(&KeyEvent::new(KeyCode::Down, KeyModifiers::empty()).into())
                .unwrap(),
            &Command::Cursor(CursorCommand::Last)
        );

        assert_eq!(
            view_model.theme.get(StatusBar::Command),
            Style::default().bg(Color::Yellow)
        );
        assert_eq!(
            view_model.theme.get(StatusBar::Empty),
            Style::default().bg(Color::Red)
        );
        assert_eq!(view_model.theme.get(List::Divider), Style::default());
    }
}
