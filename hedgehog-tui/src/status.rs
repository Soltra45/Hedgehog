use crate::scrolling::DataView;
use chrono::{DateTime, Local};
use hedgehog_player::volume::Volume;
use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

pub(crate) const TTL_LONG: Duration = Duration::from_secs(10);
pub(crate) const TTL_SHORT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorType {
    Playback,
    Database,
    Update,
    Actix,
    Command,
    IO,
}

impl ErrorType {
    fn as_str(&self) -> &'static str {
        match self {
            ErrorType::Playback => "Playback error",
            ErrorType::Database => "Internal erorr (database)",
            ErrorType::Update => "Update error",
            ErrorType::Actix => "Internal error",
            ErrorType::Command => "Invalid command",
            ErrorType::IO => "I/O error",
        }
    }

    fn ttl(&self) -> Option<Duration> {
        match self {
            ErrorType::Playback => Some(TTL_LONG),
            ErrorType::Database => None,
            ErrorType::Update => Some(TTL_SHORT),
            ErrorType::Actix => None,
            ErrorType::Command => Some(TTL_SHORT),
            ErrorType::IO => None,
        }
    }

    fn store_in_log(&self) -> bool {
        match self {
            ErrorType::Playback => true,
            ErrorType::Database => true,
            ErrorType::Update => true,
            ErrorType::Actix => true,
            ErrorType::Command => false,
            ErrorType::IO => true,
        }
    }
}

pub(crate) trait HedgehogError: fmt::Display {
    fn error_type(&self) -> ErrorType;
}

macro_rules! impl_hedgehog_error {
    ($type:ty, $error_type:expr) => {
        impl HedgehogError for $type {
            fn error_type(&self) -> ErrorType {
                $error_type
            }
        }
    };
}

impl<'a> HedgehogError for cmd_parser::ParseError<'a> {
    fn error_type(&self) -> ErrorType {
        ErrorType::Command
    }
}

impl HedgehogError for crate::cmdreader::Error {
    fn error_type(&self) -> ErrorType {
        match self {
            crate::cmdreader::Error::Io(_) => ErrorType::IO,
            crate::cmdreader::Error::Parsing(_, _) => ErrorType::Command,
        }
    }
}

impl_hedgehog_error!(hedgehog_player::GstError, ErrorType::Playback);
impl_hedgehog_error!(hedgehog_library::FeedUpdateError, ErrorType::Update);
impl_hedgehog_error!(actix::MailboxError, ErrorType::Actix);
impl_hedgehog_error!(hedgehog_library::QueryError, ErrorType::Database);
impl_hedgehog_error!(std::io::Error, ErrorType::IO);

pub(crate) struct CustomStatus {
    text: Cow<'static, str>,
    severity: Severity,
    ttl: Option<Duration>,
}

impl CustomStatus {
    pub(crate) fn new(text: impl Into<Cow<'static, str>>) -> Self {
        CustomStatus {
            text: text.into(),
            severity: Severity::Information,
            ttl: None,
        }
    }

    pub(crate) fn set_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub(crate) fn set_ttl(mut self, ttl: impl Into<Option<Duration>>) -> Self {
        self.ttl = ttl.into();
        self
    }
}

pub(crate) enum Status {
    Error(Box<dyn HedgehogError + 'static>),
    Custom(CustomStatus),
    VolumeChanged(Option<Volume>),
}

impl Status {
    pub(crate) fn error(error: impl HedgehogError + 'static) -> Self {
        Status::Error(Box::new(error))
    }

    pub(crate) fn severity(&self) -> Severity {
        match self {
            Status::Error(_) => Severity::Error,
            Status::Custom(status) => status.severity,
            Status::VolumeChanged(_) => Severity::Information,
        }
    }

    pub(crate) fn ttl(&self) -> Option<Duration> {
        match self {
            Status::Error(err) => err.error_type().ttl(),
            Status::Custom(custom) => custom.ttl,
            Status::VolumeChanged(_) => Some(Duration::from_secs(2)),
        }
    }

    pub(crate) fn variant_label(&self) -> Option<&'static str> {
        match self {
            Status::Error(error) => Some(error.error_type().as_str()),
            _ => None,
        }
    }

    fn store_in_log(&self) -> bool {
        match self {
            Status::Error(error) => error.error_type().store_in_log(),
            Status::Custom(custom) => custom.severity == Severity::Error,
            Status::VolumeChanged(_) => false,
        }
    }
}

impl From<CustomStatus> for Status {
    fn from(status: CustomStatus) -> Self {
        Status::Custom(status)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Error(value) => f.write_fmt(format_args!("{}", value)),
            Status::Custom(custom) => f.write_str(&custom.text),
            Status::VolumeChanged(Some(volume)) => {
                f.write_fmt(format_args!("Volume: {:.0}%", volume.cubic() * 100.0))
            }
            Status::VolumeChanged(None) => f.write_str("Playback muted"),
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
    Information,
}

impl Severity {
    pub(crate) fn enumerate() -> impl IntoIterator<Item = Self> {
        [Severity::Error, Severity::Warning, Severity::Information]
    }
}

enum StatusDisplay {
    LastLog,
    DisplayOnly(Status),
    None,
}

impl Default for StatusDisplay {
    fn default() -> Self {
        StatusDisplay::None
    }
}

pub(crate) struct StatusLogEntry {
    status: Status,
    timestamp: DateTime<Local>,
}

impl StatusLogEntry {
    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    pub(crate) fn timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }
}

#[derive(Default)]
pub(crate) struct StatusLog {
    log: Vec<StatusLogEntry>,
    display_status: StatusDisplay,
}

impl StatusLog {
    pub(crate) fn is_empty(&self) -> bool {
        self.log.is_empty()
    }

    pub(crate) fn push(&mut self, status: Status) {
        self.display_status = if status.store_in_log() {
            self.log.push(StatusLogEntry {
                status,
                timestamp: Local::now(),
            });
            StatusDisplay::LastLog
        } else {
            StatusDisplay::DisplayOnly(status)
        }
    }

    pub(crate) fn display_status(&self) -> Option<&Status> {
        match self.display_status {
            StatusDisplay::LastLog => self.log.last().map(|entry| &entry.status),
            StatusDisplay::DisplayOnly(ref status) => Some(status),
            StatusDisplay::None => None,
        }
    }

    pub(crate) fn clear_display(&mut self) {
        self.display_status = StatusDisplay::None;
    }

    pub(crate) fn has_errors(&self) -> bool {
        self.display_status()
            .map(|status| status.severity() == Severity::Error)
            .unwrap_or(false)
    }
}

impl DataView for StatusLog {
    type Item = StatusLogEntry;

    fn size(&self) -> usize {
        self.log.len()
    }

    fn item_at(&self, index: usize) -> Option<&Self::Item> {
        self.log.get(self.log.size().saturating_sub(index + 1))
    }

    fn find(&self, p: impl Fn(&Self::Item) -> bool) -> Option<usize> {
        self.log
            .iter()
            .enumerate()
            .find(|(_, item)| p(item))
            .map(|(index, _)| index)
    }
}
