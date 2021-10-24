use crate::dataview::{
    DataProvider, ListDataRequest, PaginatedDataMessage, PaginatedDataRequest, Versioned,
};
use crate::events::key;
use crate::history::CommandsHistory;
use crate::theming;
use crate::view_model::{Command, FocusedPane, ViewModel};
use crate::widgets::command::{CommandActionResult, CommandEditor, CommandState};
use crate::widgets::list::{List, ListItemRenderingDelegate};
use actix::prelude::*;
use crossterm::event::Event;
use hedgehog_library::{
    EpisodeSummariesQuery, FeedSummariesQuery, Library, PagedQueryRequest, QueryRequest,
    SizeRequest,
};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::text::Span;
use tui::widgets::{Block, Borders, Paragraph, Widget};
use tui::Terminal;

pub(crate) struct UI {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    command: Option<CommandState>,
    commands_history: CommandsHistory,
    library: Addr<Library>,
    view_model: ViewModel,
}

impl UI {
    pub(crate) fn new(
        size: (u16, u16),
        terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
        library: Addr<Library>,
    ) -> Self {
        UI {
            terminal,
            command: None,
            commands_history: CommandsHistory::new(),
            library,
            view_model: ViewModel::new(size),
        }
    }

    fn render(&mut self) {
        let command = &mut self.command;
        let history = &self.commands_history;
        let episodes_list = &self.view_model.episodes_list;
        let view_model = &self.view_model;

        let draw = |f: &mut tui::Frame<CrosstermBackend<std::io::Stdout>>| {
            let area = f.size();
            let library_rect = Rect::new(0, 0, area.width, area.height - 1);

            let layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(24), Constraint::Percentage(75)].as_ref())
                .split(library_rect);

            let feeds_border = Block::default()
                .borders(Borders::RIGHT)
                .border_style(view_model.theme.get(theming::List::Divider));
            let feeds_area = feeds_border.inner(layout[0]);
            f.render_widget(feeds_border, layout[0]);

            if let Some(iter) = view_model.feeds_list.iter() {
                f.render_widget(
                    List::new(
                        FeedsListRowRenderer::new(
                            &view_model.theme,
                            view_model.focus == FocusedPane::FeedsList,
                        ),
                        iter,
                    ),
                    feeds_area,
                );
            }
            if let Some(iter) = episodes_list.iter() {
                f.render_widget(
                    List::new(
                        EpisodesListRowRenderer::new(
                            &view_model.theme,
                            view_model.focus == FocusedPane::EpisodesList,
                        ),
                        iter,
                    ),
                    layout[1],
                );
            }

            let status_rect = Rect::new(0, area.height - 1, area.width, 1);
            if let Some(ref mut command_state) = command {
                let style = view_model.theme.get(theming::StatusBar::Command);
                let prompt_style = view_model.theme.get(theming::StatusBar::CommandPrompt);
                CommandEditor::new(command_state)
                    .prefix(Span::styled(":", prompt_style))
                    .style(style)
                    .render(f, status_rect, history);
            } else if let Some(status) = &view_model.status {
                let theme_selector = theming::StatusBar::Status(Some(status.severity()));
                let style = view_model.theme.get(theme_selector);
                f.render_widget(Paragraph::new(status.to_string()).style(style), status_rect);
            } else {
                f.render_widget(
                    Block::default().style(view_model.theme.get(theming::StatusBar::Empty)),
                    status_rect,
                );
            }
        };
        self.terminal.draw(draw).unwrap();
    }
}

impl Actor for UI {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.add_stream(crossterm::event::EventStream::new());
        self.view_model
            .episodes_list
            .set_provider(EpisodesListProvider {
                query: EpisodeSummariesQuery { feed_id: None },
                actor: ctx.address(),
            });
        self.view_model.feeds_list.set_provider(FeedsListProvider {
            query: FeedSummariesQuery,
            actor: ctx.address(),
        });
        self.view_model.init_rc();
        self.render();
    }
}

impl StreamHandler<crossterm::Result<crossterm::event::Event>> for UI {
    fn handle(
        &mut self,
        item: crossterm::Result<crossterm::event::Event>,
        _ctx: &mut Self::Context,
    ) {
        let event = match item {
            Ok(Event::Resize(width, height)) => {
                self.view_model.set_size(width, height);
                self.render();
                return;
            }
            Ok(event) => event,
            Err(_) => {
                System::current().stop();
                return;
            }
        };

        let should_render = match self.command {
            None => match event {
                key!('c', CONTROL) => self.view_model.handle_command_interactive(Command::Quit),
                key!(':') => {
                    self.view_model.clear_status();
                    self.command = Some(CommandState::default());
                    true
                }
                crossterm::event::Event::Key(key_event) => {
                    match self.view_model.key_mapping.get(&key_event.into()) {
                        Some(command) => {
                            let command = command.clone();
                            self.view_model.handle_command_interactive(command)
                        }
                        None => false,
                    }
                }
                _ => false,
            },
            Some(ref mut command_state) => {
                match command_state.handle_event(event, &self.commands_history) {
                    CommandActionResult::None => false,
                    CommandActionResult::Update => true,
                    CommandActionResult::Clear => {
                        self.command = None;
                        true
                    }
                    CommandActionResult::Submit => {
                        let command_str = command_state.as_str(&self.commands_history).to_string();
                        self.commands_history.push(&command_str);
                        self.command = None;
                        self.view_model.handle_command_str(command_str.as_str());
                        true
                    }
                }
            }
        };
        if should_render {
            self.render();
        }
    }
}

pub(crate) struct EpisodesListProvider {
    query: EpisodeSummariesQuery,
    actor: Addr<UI>,
}

impl DataProvider for EpisodesListProvider {
    type Request = PaginatedDataRequest;

    fn request(&self, request: crate::dataview::Versioned<Self::Request>) {
        self.actor
            .do_send(DataFetchingRequest::Episodes(self.query.clone(), request));
    }
}

pub(crate) struct FeedsListProvider {
    query: FeedSummariesQuery,
    actor: Addr<UI>,
}

impl DataProvider for FeedsListProvider {
    type Request = ListDataRequest;

    fn request(&self, request: Versioned<Self::Request>) {
        self.actor
            .do_send(DataFetchingRequest::Feeds(self.query.clone(), request));
    }
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
enum DataFetchingRequest {
    Episodes(EpisodeSummariesQuery, Versioned<PaginatedDataRequest>),
    Feeds(FeedSummariesQuery, Versioned<ListDataRequest>),
}

impl Handler<DataFetchingRequest> for UI {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, msg: DataFetchingRequest, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            DataFetchingRequest::Episodes(query, request) => {
                let version = request.version();
                match request.unwrap() {
                    PaginatedDataRequest::Size => {
                        Box::pin(self.library.send(SizeRequest(query)).into_actor(self).map(
                            move |size, actor, _ctx| {
                                let should_render = (actor.view_model.episodes_list).handle_data(
                                    Versioned::new(PaginatedDataMessage::Size(
                                        size.unwrap().unwrap(),
                                    ))
                                    .with_version(version),
                                );
                                if should_render {
                                    actor.render();
                                }
                            },
                        ))
                    }
                    PaginatedDataRequest::Page { index, range } => Box::pin(
                        self.library
                            .send(PagedQueryRequest {
                                data: query,
                                offset: range.start,
                                count: range.len(),
                            })
                            .into_actor(self)
                            .map(move |data, actor, _ctx| {
                                let should_render = (actor.view_model.episodes_list).handle_data(
                                    Versioned::new(PaginatedDataMessage::Page {
                                        index,
                                        values: data.unwrap().unwrap(),
                                    })
                                    .with_version(version),
                                );
                                if should_render {
                                    actor.render();
                                }
                            }),
                    ),
                }
            }
            DataFetchingRequest::Feeds(query, request) => {
                Box::pin(self.library.send(QueryRequest(query)).into_actor(self).map(
                    move |data, actor, _ctx| {
                        let should_render = (actor.view_model.feeds_list)
                            .handle_data(request.with_data(data.unwrap().unwrap()));
                        if should_render {
                            actor.render();
                        }
                    },
                ))
            }
        }
    }
}

struct EpisodesListRowRenderer<'t> {
    theme: &'t theming::Theme,
    default_item_state: theming::ListState,
}

impl<'t> EpisodesListRowRenderer<'t> {
    fn new(theme: &'t theming::Theme, is_focused: bool) -> Self {
        EpisodesListRowRenderer {
            theme,
            default_item_state: if is_focused {
                theming::ListState::FOCUSED
            } else {
                theming::ListState::empty()
            },
        }
    }
}

impl<'t, 'a> ListItemRenderingDelegate<'a> for EpisodesListRowRenderer<'t> {
    type Item = (Option<&'a hedgehog_library::model::EpisodeSummary>, bool);

    fn render_item(&self, area: Rect, item: Self::Item, buf: &mut tui::buffer::Buffer) {
        let (item, selected) = item;

        let mut item_state = self.default_item_state;
        if selected {
            item_state |= theming::ListState::SELECTED;
        }
        let subitem = match item.map(|item| item.title.is_some()) {
            Some(false) => Some(theming::ListSubitem::MissingTitle),
            _ => None,
        };
        let style = self.theme.get(theming::List::Item(item_state, subitem));

        buf.set_style(Rect::new(area.x, area.y, 1, area.height), style);
        buf.set_style(
            Rect::new(area.x + area.width - 1, area.y, 1, area.height),
            style,
        );

        let inner_area = Rect::new(area.x + 1, area.y, area.width - 2, area.height);
        match item {
            Some(item) => {
                let paragraph =
                    Paragraph::new(item.title.as_deref().unwrap_or("no title")).style(style);
                paragraph.render(inner_area, buf);
            }
            None => buf.set_string(area.x, area.y, " . . . ", style),
        }
    }
}

struct FeedsListRowRenderer<'t> {
    theme: &'t theming::Theme,
    default_item_state: theming::ListState,
}

impl<'t> FeedsListRowRenderer<'t> {
    fn new(theme: &'t theming::Theme, is_focused: bool) -> Self {
        FeedsListRowRenderer {
            theme,
            default_item_state: if is_focused {
                theming::ListState::FOCUSED
            } else {
                theming::ListState::empty()
            },
        }
    }
}

impl<'t, 'a> ListItemRenderingDelegate<'a> for FeedsListRowRenderer<'t> {
    type Item = (Option<&'a hedgehog_library::model::FeedSummary>, bool);

    fn render_item(&self, area: Rect, item: Self::Item, buf: &mut tui::buffer::Buffer) {
        let (item, selected) = item;

        let mut item_state = self.default_item_state;
        if selected {
            item_state |= theming::ListState::SELECTED;
        }
        let subitem = match item.map(|item| item.has_title) {
            Some(false) => Some(theming::ListSubitem::MissingTitle),
            _ => None,
        };
        let style = self.theme.get(theming::List::Item(item_state, subitem));

        buf.set_style(Rect::new(area.x, area.y, 1, area.height), style);
        buf.set_style(
            Rect::new(area.x + area.width - 1, area.y, 1, area.height),
            style,
        );

        let inner_area = Rect::new(area.x + 1, area.y, area.width - 2, area.height);
        match item {
            Some(item) => {
                let paragraph = Paragraph::new(item.title.as_str()).style(style);
                paragraph.render(inner_area, buf);
            }
            None => buf.set_string(area.x, area.y, " . . . ", style),
        }
    }
}
