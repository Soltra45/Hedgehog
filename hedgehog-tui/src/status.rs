use hedgehog_player::volume::Volume;
use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

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
            crate::cmdreader::Error::Resolution => ErrorType::IO,
        }
    }
}

impl_hedgehog_error!(hedgehog_player::GstError, ErrorType::Playback);
impl_hedgehog_error!(hedgehog_library::FeedUpdateError, ErrorType::Update);
impl_hedgehog_error!(actix::MailboxError, ErrorType::Actix);
impl_hedgehog_error!(hedgehog_library::QueryError, ErrorType::Database);
impl_hedgehog_error!(std::io::Error, ErrorType::IO);

pub(crate) enum Status {
    Error(Box<dyn HedgehogError + 'static>),
    Custom(Cow<'static, str>, Severity),
    VolumeChanged(Option<Volume>),
}

impl Status {
    pub(crate) fn error(error: impl HedgehogError + 'static) -> Self {
        Status::Error(Box::new(error))
    }

    pub(crate) fn severity(&self) -> Severity {
        match self {
            Status::Error(_) => Severity::Error,
            Status::Custom(_, severity) => *severity,
            Status::VolumeChanged(_) => Severity::Information,
        }
    }

    pub(crate) fn new_custom(text: impl Into<Cow<'static, str>>, severity: Severity) -> Self {
        Status::Custom(text.into(), severity)
    }

    pub(crate) fn ttl(&self) -> Option<Duration> {
        match self {
            Status::Error(_) => None,
            Status::Custom(_, _) => None,
            Status::VolumeChanged(_) => Some(Duration::from_secs(2)),
        }
    }

    pub(crate) fn variant_label(&self) -> Option<&'static str> {
        match self {
            Status::Error(error) => Some(error.error_type().as_str()),
            _ => None,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Error(value) => f.write_fmt(format_args!("{}", value)),
            Status::Custom(error, _) => f.write_str(error),
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

#[derive(Default)]
pub(crate) struct StatusLog {
    display_status: Option<Status>,
}

impl StatusLog {
    pub(crate) fn push(&mut self, status: Status) {
        self.display_status = Some(status);
    }

    pub(crate) fn display_status(&self) -> Option<&Status> {
        self.display_status.as_ref()
    }

    pub(crate) fn clear_display(&mut self) {
        self.display_status = None;
    }

    pub(crate) fn has_errors(&self) -> bool {
        self.display_status
            .as_ref()
            .map(|status| status.severity() == Severity::Error)
            .unwrap_or(false)
    }
}
