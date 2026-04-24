use axum::{Router, body::Body, extract::ConnectInfo, http::Request, routing::get};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tower_http::trace::TraceLayer;
use tracing::{Subscriber, info_span};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

#[derive(Clone)]
struct FieldCaptureLayer {
    captured_ip: Arc<Mutex<Option<String>>>,
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for FieldCaptureLayer {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("Span should exist");
        if span.name() == "request" {
            let mut visitor = IpVisitor { ip: None };
            attrs.record(&mut visitor);
            if let Some(ip) = visitor.ip {
                let mut captured = self.captured_ip.lock().unwrap();
                *captured = Some(ip);
            }
        }
    }
}

struct IpVisitor {
    ip: Option<String>,
}

impl tracing::field::Visit for IpVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "client_ip" {
            self.ip = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "client_ip" {
            self.ip = Some(value.to_string());
        }
    }
}

#[tokio::test]
async fn test_client_ip_logging() {
    use tracing_subscriber::prelude::*;

    let captured_ip = Arc::new(Mutex::new(None));
    let layer = FieldCaptureLayer {
        captured_ip: captured_ip.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    // Test with actual IP
    let addr: SocketAddr = "1.2.3.4:5678".parse().unwrap();
    let req = Request::builder()
        .uri("/")
        .extension(ConnectInfo(addr))
        .body(Body::empty())
        .unwrap();

    let app = Router::new().route("/", get(|| async { "ok" })).layer(
        TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
            let remote_addr = request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            info_span!(
                "request",
                client_ip = %remote_addr,
            )
        }),
    );

    app.oneshot(req).await.unwrap();

    let ip = captured_ip.lock().unwrap().take();
    assert_eq!(ip, Some("1.2.3.4:5678".to_string()));

    // Test with unknown IP
    let req_unknown = Request::builder().uri("/").body(Body::empty()).unwrap();

    let app_unknown = Router::new().route("/", get(|| async { "ok" })).layer(
        TraceLayer::new_for_http().make_span_with(|request: &Request<Body>| {
            let remote_addr = request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            info_span!(
                "request",
                client_ip = %remote_addr,
            )
        }),
    );

    app_unknown.oneshot(req_unknown).await.unwrap();

    let ip_unknown = captured_ip.lock().unwrap().take();
    assert_eq!(ip_unknown, Some("unknown".to_string()));
}
