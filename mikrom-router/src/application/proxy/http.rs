use pingora::prelude::*;

pub(super) async fn write_text_response(
    session: &mut Session,
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
    end_stream: bool,
) -> pingora::prelude::Result<bool> {
    let mut response = ResponseHeader::build(status, Some(body.len()))?;
    crate::application::proxy::set_router_server_header(&mut response)?;
    for (key, value) in headers {
        response.insert_header((*key).to_string(), (*value).to_string())?;
    }

    session
        .write_response_header(Box::new(response), end_stream)
        .await?;
    session
        .write_response_body(Some(body.to_string().into()), true)
        .await?;
    Ok(true)
}
