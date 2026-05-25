use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("API error: {message} (HTTP {status})")]
    Api { status: u16, message: String },

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Not found: {resource} '{id}'")]
    NotFound { resource: String, id: String },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Input required")]
    MissingInput,

    #[error("Operation cancelled")]
    Cancelled,
}

pub type CliResult<T> = Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_display_includes_status_and_message() {
        let err = CliError::Api {
            status: 500,
            message: "internal failure".to_string(),
        };
        assert_eq!(err.to_string(), "API error: internal failure (HTTP 500)");
    }

    #[test]
    fn unauthorized_display() {
        let err = CliError::Unauthorized("token expired".to_string());
        assert_eq!(err.to_string(), "Unauthorized: token expired");
    }

    #[test]
    fn not_found_display() {
        let err = CliError::NotFound {
            resource: "app".to_string(),
            id: "my-app".to_string(),
        };
        assert_eq!(err.to_string(), "Not found: app 'my-app'");
    }

    #[test]
    fn validation_display() {
        let err = CliError::Validation("bad input".to_string());
        assert_eq!(err.to_string(), "Validation error: bad input");
    }

    #[test]
    fn config_display() {
        let err = CliError::Config("missing key".to_string());
        assert_eq!(err.to_string(), "Configuration error: missing key");
    }

    #[test]
    fn missing_input_display() {
        assert_eq!(CliError::MissingInput.to_string(), "Input required");
    }

    #[test]
    fn cancelled_display() {
        assert_eq!(CliError::Cancelled.to_string(), "Operation cancelled");
    }

    #[test]
    fn from_io_error() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let err: CliError = io.into();
        assert!(matches!(err, CliError::Io(_)));
        assert!(err.to_string().contains("file gone"));
    }

    #[test]
    fn from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: CliError = json_err.into();
        assert!(matches!(err, CliError::Json(_)));
    }

    #[test]
    fn from_toml_error() {
        let toml_err = "[[[bad".parse::<toml::Value>().unwrap_err();
        let err: CliError = toml_err.into();
        assert!(matches!(err, CliError::Toml(_)));
    }
}
