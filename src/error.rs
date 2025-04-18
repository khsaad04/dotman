use std::io;

pub type Result<T> = std::result::Result<T, Error>;

pub struct Error {
    ctx: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.ctx)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self { ctx: value }
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self {
            ctx: value.to_string(),
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self {
            ctx: format!("{value}"),
        }
    }
}
