use hedgehog_player::volume::Volume;
use std::borrow::Cow;
use std::fmt;

#[derive(Debug)]
pub(crate) enum Status {
    CommandParsingError(cmd_parser::ParseError<'static>),
    Custom(Cow<'static, str>, Severity),
    VolumeChanged(Option<Volume>),
}

impl Status {
    pub(crate) fn severity(&self) -> Severity {
        match self {
            Status::CommandParsingError(_) => Severity::Error,
            Status::Custom(_, severity) => *severity,
            Status::VolumeChanged(_) => Severity::Information,
        }
    }

    pub(crate) fn new_custom(text: impl Into<Cow<'static, str>>, severity: Severity) -> Self {
        Status::Custom(text.into(), severity)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::CommandParsingError(error) => {
                f.write_fmt(format_args!("Invalid command: {}", error))
            }
            Status::Custom(error, _) => f.write_str(error),
            Status::VolumeChanged(Some(volume)) => {
                f.write_fmt(format_args!("Volume: {:.0}%", volume.cubic() * 100.0))
            }
            Status::VolumeChanged(None) => f.write_str("Playback muted"),
        }
    }
}

impl From<cmd_parser::ParseError<'static>> for Status {
    fn from(error: cmd_parser::ParseError<'static>) -> Self {
        Status::CommandParsingError(error)
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
