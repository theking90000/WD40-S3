use std::{
    io,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use openidconnect::{
    AccessToken, AdditionalClaims, AdditionalProviderMetadata, ClientId, ClientSecret,
    EndpointNotSet, EndpointSet, IntrospectionUrl, IssuerUrl, ProviderMetadata, RequestTokenError,
    SubjectIdentifier, TokenIntrospectionResponse, UserInfoClaims, UserInfoError,
    core::{
        CoreAuthDisplay, CoreClaimName, CoreClaimType, CoreClient, CoreClientAuthMethod,
        CoreGenderClaim, CoreGrantType, CoreJsonWebKey, CoreJweContentEncryptionAlgorithm,
        CoreJweKeyManagementAlgorithm, CoreResponseMode, CoreResponseType,
        CoreSubjectIdentifierType, CoreTokenIntrospectionResponse, CoreTokenType,
    },
    reqwest,
};
use serde::{Deserialize, Serialize};

use super::AuthError;

/// Runtime OIDC configuration. The caller owns secrets and environment loading.
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
}

/// Identity asserted by the configured provider, available in request extensions.
#[derive(Clone, Debug, Serialize)]
pub struct CurrentUser {
    pub issuer: String,
    pub subject: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub roles: Vec<String>,
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ExtraMetadata {
    introspection_endpoint: IntrospectionUrl,
    #[serde(default)]
    code_challenge_methods_supported: Option<Vec<String>>,
}

impl AdditionalProviderMetadata for ExtraMetadata {}

type Metadata = ProviderMetadata<
    ExtraMetadata,
    CoreAuthDisplay,
    CoreClientAuthMethod,
    CoreClaimName,
    CoreClaimType,
    CoreGrantType,
    CoreJweContentEncryptionAlgorithm,
    CoreJweKeyManagementAlgorithm,
    CoreJsonWebKey,
    CoreResponseMode,
    CoreResponseType,
    CoreSubjectIdentifierType,
>;

type OidcClient =
    CoreClient<EndpointSet, EndpointNotSet, EndpointSet, EndpointNotSet, EndpointSet, EndpointSet>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ExtraClaims {
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    groups: Vec<String>,
}

impl AdditionalClaims for ExtraClaims {}

pub(crate) struct Oidc {
    pub(super) client: OidcClient,
    pub(super) http: reqwest::Client,
    issuer: IssuerUrl,
}

impl Oidc {
    pub(crate) async fn new(config: OidcConfig) -> io::Result<Self> {
        if config.client_id.is_empty() || config.client_secret.is_empty() {
            return Err(io::Error::other("OIDC client ID and secret are required"));
        }
        let issuer = IssuerUrl::new(config.issuer_url).map_err(io::Error::other)?;
        super::cookies::redirect_uri(issuer.as_str().to_owned())
            .map_err(|_| io::Error::other("OIDC issuer must use HTTPS, or HTTP on loopback"))?;
        if issuer.url().query().is_some() {
            return Err(io::Error::other("OIDC issuer must not contain a query"));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(io::Error::other)?;
        let metadata = Metadata::discover_async(issuer.clone(), &http)
            .await
            .map_err(|error| io::Error::other(format!("OIDC discovery failed: {error}")))?;
        if metadata
            .additional_metadata()
            .code_challenge_methods_supported
            .as_ref()
            .is_some_and(|methods| !methods.iter().any(|method| method == "S256"))
        {
            return Err(io::Error::other("OIDC provider does not support PKCE S256"));
        }
        let token = metadata
            .token_endpoint()
            .cloned()
            .ok_or_else(|| io::Error::other("OIDC provider has no token endpoint"))?;
        let userinfo = metadata
            .userinfo_endpoint()
            .cloned()
            .ok_or_else(|| io::Error::other("OIDC provider has no UserInfo endpoint"))?;
        let introspection = metadata
            .additional_metadata()
            .introspection_endpoint
            .clone();
        for endpoint in [
            metadata.authorization_endpoint().as_str(),
            token.as_str(),
            userinfo.as_str(),
            introspection.as_str(),
            metadata.jwks_uri().as_str(),
        ] {
            super::cookies::redirect_uri(endpoint.to_owned()).map_err(|_| {
                io::Error::other("OIDC endpoints must use HTTPS, or HTTP on loopback")
            })?;
        }
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.client_id),
            Some(ClientSecret::new(config.client_secret)),
        )
        .set_token_uri(token)
        .set_user_info_url(userinfo)
        .set_introspection_url(introspection);
        Ok(Self {
            client,
            http,
            issuer,
        })
    }

    pub(super) async fn introspect(
        &self,
        token: &AccessToken,
    ) -> Result<CoreTokenIntrospectionResponse, AuthError> {
        let info = self
            .client
            .introspect(token)
            .request_async(&self.http)
            .await
            .map_err(|error| match error {
                RequestTokenError::Request(_) => AuthError::unavailable(),
                _ => AuthError::upstream(),
            })?;
        if !info.active() {
            return Err(AuthError::unauthorized());
        }
        let client_id = self.client.client_id();
        let client_matches = info.client_id().is_some_and(|value| value == client_id);
        let audience_matches = info
            .aud()
            .is_some_and(|values| values.iter().any(|value| value == client_id.as_str()));
        if !client_matches && !audience_matches
            || info.client_id().is_some_and(|value| value != client_id)
            || info.aud().is_some_and(|_| !audience_matches)
            || info
                .iss()
                .is_some_and(|value| value != self.issuer.as_str())
            || info
                .token_type()
                .is_some_and(|value| value != &CoreTokenType::Bearer)
            || info.sub().is_none_or(str::is_empty)
        {
            return Err(AuthError::unauthorized());
        }
        let now = unix_seconds()?;
        let expiration = info.exp().ok_or_else(AuthError::upstream)?;
        if expiration.timestamp() <= now || info.nbf().is_some_and(|value| value.timestamp() > now)
        {
            return Err(AuthError::unauthorized());
        }
        Ok(info)
    }

    pub(super) async fn current_user(&self, token: AccessToken) -> Result<CurrentUser, AuthError> {
        // ponytail: two provider calls per request; add a bounded cache only if needed.
        let info = self.introspect(&token).await?;
        let subject = info.sub().ok_or_else(AuthError::unauthorized)?;
        let claims: UserInfoClaims<ExtraClaims, CoreGenderClaim> = self
            .client
            .user_info(token, Some(SubjectIdentifier::new(subject.to_owned())))
            .request_async(&self.http)
            .await
            .map_err(|error| match error {
                UserInfoError::Request(_) => AuthError::unavailable(),
                UserInfoError::Response(status, _, _)
                    if status == reqwest::StatusCode::UNAUTHORIZED
                        || status == reqwest::StatusCode::FORBIDDEN =>
                {
                    AuthError::unauthorized()
                }
                UserInfoError::ClaimsVerification(_) => AuthError::unauthorized(),
                _ => AuthError::upstream(),
            })?;
        let name = claims
            .name()
            .and_then(|names| {
                names
                    .get(None)
                    .or_else(|| names.iter().next().map(|(_, value)| value))
            })
            .map(|value| value.as_str().to_owned());
        Ok(CurrentUser {
            issuer: self.issuer.as_str().to_owned(),
            subject: claims.subject().as_str().to_owned(),
            name,
            email: claims.email().map(|value| value.as_str().to_owned()),
            email_verified: claims.email_verified(),
            roles: claims.additional_claims().roles.clone(),
            groups: claims.additional_claims().groups.clone(),
        })
    }
}

pub(super) fn unix_seconds() -> Result<i64, AuthError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AuthError::upstream())?
        .as_secs();
    i64::try_from(seconds).map_err(|_| AuthError::upstream())
}
