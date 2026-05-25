use crate::domain::error::CliError;
use reqwest::StatusCode;

pub fn map_http_error(status: StatusCode, message: String) -> CliError {
    match status {
        StatusCode::UNAUTHORIZED => CliError::Unauthorized(message),
        StatusCode::FORBIDDEN => CliError::Unauthorized(message),
        StatusCode::NOT_FOUND => CliError::NotFound {
            resource: "resource".to_string(),
            id: "unknown".to_string(),
        },
        StatusCode::CONFLICT => CliError::Validation(message),
        StatusCode::BAD_REQUEST => CliError::Validation(message),
        _ => CliError::Api {
            status: status.as_u16(),
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_401_to_unauthorized() {
        let err = map_http_error(StatusCode::UNAUTHORIZED, "bad creds".to_string());
        assert!(matches!(err, CliError::Unauthorized(msg) if msg == "bad creds"));
    }

    #[test]
    fn maps_403_to_unauthorized() {
        let err = map_http_error(StatusCode::FORBIDDEN, "no access".to_string());
        assert!(matches!(err, CliError::Unauthorized(msg) if msg == "no access"));
    }

    #[test]
    fn maps_404_to_not_found() {
        let err = map_http_error(StatusCode::NOT_FOUND, "gone".to_string());
        assert!(matches!(err, CliError::NotFound { .. }));
    }

    #[test]
    fn maps_409_to_validation() {
        let err = map_http_error(StatusCode::CONFLICT, "already exists".to_string());
        assert!(matches!(err, CliError::Validation(msg) if msg == "already exists"));
    }

    #[test]
    fn maps_400_to_validation() {
        let err = map_http_error(StatusCode::BAD_REQUEST, "invalid".to_string());
        assert!(matches!(err, CliError::Validation(msg) if msg == "invalid"));
    }

    #[test]
    fn maps_500_to_api_error() {
        let err = map_http_error(StatusCode::INTERNAL_SERVER_ERROR, "boom".to_string());
        assert!(matches!(err, CliError::Api { status: 500, message } if message == "boom"));
    }

    #[test]
    fn maps_503_to_api_error() {
        let err = map_http_error(StatusCode::SERVICE_UNAVAILABLE, "down".to_string());
        assert!(matches!(err, CliError::Api { status: 503, message } if message == "down"));
    }

    #[test]
    fn maps_422_to_api_error() {
        let err = map_http_error(StatusCode::UNPROCESSABLE_ENTITY, "nope".to_string());
        assert!(matches!(err, CliError::Api { status: 422, message } if message == "nope"));
    }
}
