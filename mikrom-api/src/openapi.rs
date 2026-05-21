use rovo::aide::openapi::{Components, Info, OpenApi, ReferenceOr, SecurityScheme};

pub fn build_openapi() -> OpenApi {
    let mut openapi = OpenApi {
        info: Info {
            title: "Mikrom API".to_string(),
            version: "0.3.0".to_string(),
            description: Some("REST API for Mikrom microVM orchestration".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut components = Components::default();
    components.security_schemes.insert(
        "jwt".to_string(),
        ReferenceOr::Item(SecurityScheme::Http {
            scheme: "bearer".to_string(),
            bearer_format: Some("JWT".to_string()),
            description: Some("JWT authorization header using the Bearer scheme.".to_string()),
            extensions: Default::default(),
        }),
    );
    openapi.components = Some(components);

    openapi
}
