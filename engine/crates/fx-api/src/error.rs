#[derive(Debug)]
pub enum HttpError {
    NoTailscale(String),
    MissingBearerToken,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTailscale(msg) => write!(f, "{msg}"),
            Self::MissingBearerToken => write!(
                f,
                "HTTP API requires a bearer token for authentication.\n\n\
                 Option 1 (recommended): Use the TUI command:\n\
                 \x20 /auth http set-bearer <TOKEN>\n\n\
                 Option 2 (deprecated): Add to ~/.fawx/config.toml:\n\
                 \x20 [http]\n\
                 \x20 bearer_token = \"your-secret-token\"\n\n\
                 Generate a token with: openssl rand -hex 32"
            ),
        }
    }
}

impl std::error::Error for HttpError {}
