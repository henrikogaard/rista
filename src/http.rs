//! HTTP transport for GPUI: the `gpui-pre` snapshot ships no real client —
//! its app-wide `http_client` is a stub that fails every request. This bridges
//! `ureq` (sync, blocking) onto `http_client::HttpClient` via `smol::unblock`
//! so remote images (`![](https://…)`, `cover:`/`banner:` URLs) can load.

use gpui_kit::http_client::http::{self, HeaderValue};
use gpui_kit::http_client::{AsyncBody, HttpClient, Request, Response, Url};
use smol::io::AsyncReadExt;
use std::sync::Arc;

struct UreqClient {
    agent: ureq::Agent,
    user_agent: HeaderValue,
}

/// The app-wide HTTP client.
pub fn client() -> Arc<dyn HttpClient> {
    let config = ureq::config::Config::builder()
        // Keep non-2xx responses readable — callers check the status.
        .http_status_as_error(false)
        .build();
    Arc::new(UreqClient {
        agent: ureq::Agent::new_with_config(config),
        user_agent: HeaderValue::from_static("rista"),
    })
}

impl HttpClient for UreqClient {
    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Response<AsyncBody>>> + Send>,
    > {
        let agent = self.agent.clone();
        Box::pin(async move {
            let (parts, mut body) = req.into_parts();
            let mut body_bytes = Vec::new();
            body.read_to_end(&mut body_bytes).await?;
            smol::unblock(move || {
                let request = http::Request::from_parts(parts, body_bytes);
                let (parts, mut body) = agent.run(request)?.into_parts();
                Ok(Response::from_parts(
                    parts,
                    AsyncBody::from_bytes(body.read_to_vec()?.into()),
                ))
            })
            .await
        })
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }
}
