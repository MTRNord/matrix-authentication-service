// Copyright 2024 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use aide::{
    axum::ApiRouter,
    openapi::{OAuth2Flow, OAuth2Flows, OpenApi, SecurityScheme, Server, Tag},
};
use axum::{
    extract::{FromRef, FromRequestParts, State},
    http::HeaderName,
    response::Html,
    Json, Router,
};
use hyper::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use indexmap::IndexMap;
use mas_axum_utils::FancyError;
use mas_http::CorsLayerExt;
use mas_matrix::BoxHomeserverConnection;
use mas_router::{
    ApiDoc, ApiDocCallback, OAuth2AuthorizationEndpoint, OAuth2TokenEndpoint, Route, SimpleRoute,
    UrlBuilder,
};
use mas_storage::BoxRng;
use mas_templates::{ApiDocContext, Templates};
use tower_http::cors::{Any, CorsLayer};

mod call_context;
mod model;
mod params;
mod response;
mod schema;
mod v1;

use self::call_context::CallContext;
use crate::passwords::PasswordManager;

pub fn router<S>() -> (OpenApi, Router<S>)
where
    S: Clone + Send + Sync + 'static,
    BoxHomeserverConnection: FromRef<S>,
    PasswordManager: FromRef<S>,
    BoxRng: FromRequestParts<S>,
    CallContext: FromRequestParts<S>,
    Templates: FromRef<S>,
    UrlBuilder: FromRef<S>,
{
    aide::gen::in_context(|ctx| {
        ctx.schema = schemars::gen::SchemaGenerator::new(schemars::gen::SchemaSettings::openapi3());
    });

    let mut api = OpenApi::default();
    let router = ApiRouter::<S>::new()
        .nest("/api/admin/v1", self::v1::router())
        .finish_api_with(&mut api, |t| {
            t.title("Matrix Authentication Service admin API")
                .tag(Tag {
                    name: "oauth2-session".to_owned(),
                    description: Some("Manage OAuth2 sessions".to_owned()),
                    ..Tag::default()
                })
                .tag(Tag {
                    name: "user".to_owned(),
                    description: Some("Manage users".to_owned()),
                    ..Tag::default()
                })
                .security_scheme(
                    "oauth2",
                    SecurityScheme::OAuth2 {
                        flows: OAuth2Flows {
                            client_credentials: Some(OAuth2Flow::ClientCredentials {
                                refresh_url: Some(OAuth2TokenEndpoint::PATH.to_owned()),
                                token_url: OAuth2TokenEndpoint::PATH.to_owned(),
                                scopes: IndexMap::from([(
                                    "urn:mas:admin".to_owned(),
                                    "Grant access to the admin API".to_owned(),
                                )]),
                            }),
                            authorization_code: Some(OAuth2Flow::AuthorizationCode {
                                authorization_url: OAuth2AuthorizationEndpoint::PATH.to_owned(),
                                refresh_url: Some(OAuth2TokenEndpoint::PATH.to_owned()),
                                token_url: OAuth2TokenEndpoint::PATH.to_owned(),
                                scopes: IndexMap::from([(
                                    "urn:mas:admin".to_owned(),
                                    "Grant access to the admin API".to_owned(),
                                )]),
                            }),
                            implicit: None,
                            password: None,
                        },
                        description: None,
                        extensions: IndexMap::default(),
                    },
                )
                .security_requirement_scopes("oauth2", ["urn:mas:admin"])
        });

    let router = router
        // Serve the OpenAPI spec as JSON
        .route(
            "/api/spec.json",
            axum::routing::get({
                let api = api.clone();
                move |State(url_builder): State<UrlBuilder>| {
                    // Let's set the servers to the HTTP base URL
                    let mut api = api.clone();
                    api.servers = vec![Server {
                        url: url_builder.http_base().to_string(),
                        ..Server::default()
                    }];

                    std::future::ready(Json(api))
                }
            }),
        )
        // Serve the Swagger API reference
        .route(ApiDoc::route(), axum::routing::get(swagger))
        .route(
            ApiDocCallback::route(),
            axum::routing::get(swagger_callback),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_otel_headers([
                    AUTHORIZATION,
                    ACCEPT,
                    CONTENT_TYPE,
                    // Swagger will send this header, so we have to allow it to avoid CORS errors
                    HeaderName::from_static("x-requested-with"),
                ]),
        );

    (api, router)
}

async fn swagger(
    State(url_builder): State<UrlBuilder>,
    State(templates): State<Templates>,
) -> Result<Html<String>, FancyError> {
    let ctx = ApiDocContext::from_url_builder(&url_builder);
    let res = templates.render_swagger(&ctx)?;
    Ok(Html(res))
}

async fn swagger_callback(
    State(url_builder): State<UrlBuilder>,
    State(templates): State<Templates>,
) -> Result<Html<String>, FancyError> {
    let ctx = ApiDocContext::from_url_builder(&url_builder);
    let res = templates.render_swagger_callback(&ctx)?;
    Ok(Html(res))
}
