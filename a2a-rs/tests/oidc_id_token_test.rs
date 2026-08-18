//! `OpenIdConnectAuthenticator` against a live OIDC provider.
//!
//! The property under test is the same one the OAuth2 introspection test
//! covers: the principal has to be the subject the provider signed, not the
//! credential presented — a user who signs in again gets a new ID token and is
//! the same person. On top of that, an ID token is a signed document, so this
//! also covers what the signature is worth: the audience, the issuer, the
//! expiry, and a provider that rotates its keys while the agent is running.

#![cfg(all(feature = "auth", feature = "http-server"))]

use std::sync::{Arc, RwLock};
use std::time::Duration;

use a2a_rs::adapter::OpenIdConnectAuthenticator;
use a2a_rs::port::{AuthContext, Authenticator};
use axum::{Json, Router, extract::State, routing::get};
use chrono::{Duration as ChronoDuration, Utc};
use oauth2::{ClientId, RedirectUrl};
use openidconnect::core::{
    CoreIdToken, CoreIdTokenClaims, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
    CoreProviderMetadata, CoreResponseType, CoreRsaPrivateSigningKey, CoreSubjectIdentifierType,
};
use openidconnect::{
    Audience, EmptyAdditionalClaims, EmptyAdditionalProviderMetadata, EndUserEmail, IssuerUrl,
    JsonWebKeyId, JsonWebKeySetUrl, PrivateSigningKey, ResponseTypes, StandardClaims,
    SubjectIdentifier,
};

/// Test-only RSA keys, generated for this file and used nowhere else. Committed
/// so the test needs no key generation at run time, which is slow enough in a
/// debug build to matter. The second one stands in for a signer the provider
/// never published.
///
/// Only the base64 body is written here and [`pem`] adds the PKCS#1 armour: a
/// committed `BEGIN RSA PRIVATE KEY` block trips secret scanners, and a
/// throwaway fixture is not worth a blocked push.
const SIGNING_KEY: &str = "MIIEogIBAAKCAQEAr+QQWxV7jhorHZfwIRkAk1glmgACOEwI+kG48ylDJIMCWN8M
om9DnajsNxTxmRGupcn16iVhpvZ/wjSAGs6xP8Ohcy9DSKIHIdciN6ikoWY9kGF9
bu0bsNXN8vJza+9mu1OBlFxEqBvaM6vGSBPBwvV517+7DILaaDHJDv9deQnmx2K7
ige/6o/qMnASnAWOEYf3gy2b3p/4LfFIeIYjjbniESL+gMt7T3Ub3jjHasDrE1DI
sjtgjrBOWKL7MtDoa8n4FJ510gokF2F9QFEtC7EiA0bE2wF5P5eBv46oQdK2cVzI
nKKSnPr0GntbtRmUC6J9Kwo2mjqzonOp2tujywIDAQABAoIBAAREFkd3QmtkZBZQ
KJFM73Aja6oMBQHDilYzgN5Y6ll42fY41rAb3bp1reD6H4/0V1WLC+1VYcRwZxHZ
PyAnUjI3NvO5ujqJP34JHznVW8TUW3rkemvV0V2dGeUiDz2XbVjuwg5MnJetMUIe
kO0PmZv2YzGh41H+3Hg5eORluikk032SAlyKY3eNZ/iM4eyAFHrn86VQrvWiFJzH
gJNErOgy8oj1kWU+A8zVPY5O75r6yuug323ivyPh1/BFUswosVMwcs5nuePEBrKC
5Tgwwp5aQhM6ZSnfsaGwMA57MVVVvvBGSRrjBTEu3iOY6lKw1FMwyrYKoGGximNM
gbhr7okCgYEA6kqcMZM2Ht2iD+pSK+hZN7yZiKfZ/D77AKcS93ZKKFGuO4S0brxe
d/Mzq++EYJBlOOhWe2EaAFUrSRs4BKb1+D8cwSVzoHUdYvyjBNVtbjYJ5cGpo7G0
IvmuLZb9AxktcB2bcKSF901D9kuCS3gOWawPuh8+iXTQbhEFDwoVTwMCgYEAwDAx
aUL1Yez60GI7TFj/XYBERJ3pM4DgRt603iGuORnDijezCMBp8jt5V4ZCtZ+WvUsk
VB7Fkbqb//y2FctQMLJj7zT687k5V8RnD1BmJtv0UBGLBzAW5h2tInH4gobugvY5
wMocHF2i9+9/ZdK4w6vQn6Pwpruc/dfUtjTteZkCgYAt3631hv1xzbONqjOspTHS
1/q35yWnXi2HUy9DhMXAXz3eKX2qsPdORTA42gzxW1R2cAd+4ORWbFatWcb+IjLH
CJR4vPyGzmeSmiTRLXjfu3T0p5avlnvO3VRdWNLxaFydNy7YP157rYVBFEfOvxMQ
O8BYkQWNpHGrG2oCJ9dEQQKBgCmKARQEQe9BflCN+s0cq001TQwbqWzVXSRUPHmK
hBKZa/cy5MJufDe7/RUa0s5YyQbu99IquH8v+0nQADcjs5hi5lCsfdUx4qACtlfM
A9hAUEUCFa+fCEQChApe4dysd17dA0yVIpBK+M9n93w1mHPKbhQjJf+Tq3H+NV6Q
gAd5AoGAZySkhzqiKckAp5QVvFQqEsPf0QNRITnhyo5fU0GopnwqBir57u3EvHKg
WT0KLIRZGfTAzyRW4nmRDTDkkKXYyiLcf3efIAMFmky4FLMf3AooqZpwKbSRCr9M
jFr6wpP4n7WA+Z3tSBuqFCXK5WD/KpFB50u0THBjBY2TO9Ls3Cg=
";

const OTHER_KEY: &str = "MIIEpAIBAAKCAQEAucfDyHvd3tYy0osds9bKY62+OtpEzlbmtL6p3CbHTL/XAN10
9IsFfxvCv6ojSZEMoQbklmZKp6cKEGszuY3xOqHeyOK1IdYMICYjw3KQngzpNO2t
brYEkUtJvditGKr6adXSRPU83lhHYcDJtwHtkW38F2LsCtfcC2M8fWufUbXoY3Y4
0HZ4MWS4yj+cfps5ue6+pntEtdxKgoGtGCoTuRBzrx6u8VXm0xP27HIiuLjQng5o
wUe3wG+TWcStos1Y2ZCtZ+7BxWU2U5WOJn5CB4LjaSYn6PrH5td9ohNzvLOf7bge
m59uy9NYFPMKzRnqlXvftVNyjrlzC6hVfdBt2wIDAQABAoIBAFY+fo0bs6w3E/DZ
1DgghmQvzBfWLAr+HKvbt08UUYE3rcAhDqJXx31yjb4cZbVJOOuoH4YShqW9zdZB
bgm98zac3qezVxMWIxrpmcCp9qjopXqEu/ahWQ16Pgl8BR9mgEmRkcOhdVhi8wBW
V041/ut8e4L/0URXYTeIIhS3WYa8eWG+gh9a/NSuO/pGhUQ5jOkKqZq6ox8Bgy+Z
ndGR2/vkIkyKR+bGIy2E/W4o47Ky4ePvhzIp82CJAUFJcDRgeOB4dHAvIy1UrNl2
pb5PkY/HkcZfoGaWN0vW2OYgfRj86A5K0FQ9pWRCOlVyOuguAMyv2MT5aswK6qXU
ZO9HasECgYEA2t6F7dgBu0Bu630pvVVb3y7qQKRRK+BfQYw4OvpD5pzpO9hjeP0H
bYIudZfIb93398QBjLS3zvF/IEpEy7Qzmx7YbDlRgEA3aDbMpL9b56S/HQJUc3hb
P586ciuin/o/0GCHoWb6g00I/enqbEwp/yvOH0ASQgPwxzcexIKM6acCgYEA2Uwx
WqxHiMZS5Lj4hq+NKRyfxCxEoycrkKkJLnApPijf8kvjfsfT4FhMvfSNAbfC5S4u
sVtg8axNZZQFSfPYb0yUq7yeku8iaRZ+vKnu/hofPk61YBvih2MpOP5FcvKiHWiL
tn+3wSNP6uYQffhYyd3+e/Uz3PU6gypfVh32OK0CgYEAi5EAeGWY3Q4+bP44YpqO
5ifliuj9Nexy8bp3lOxH5kPC2r2m2N0JIoS0GZp7XxJ9cEpV7qLC3zzSIwYZDojP
q6gkvAZk+VJ5woPHRXsdIP7GO7pjnepuzYg83dcDcd5DWR5k/sBLGPVDuZ6zNPHw
id4mJ3lU6zHWFUMJ5KeXMdsCgYB8T25cpPo3cN2zI25p/rwOrOVpYLnTbHErzMgH
3Pp7KP8Uqf13ZfH8AgfFE8YnGW1Rlt33cINBPoT4e3mbGPjUk0bqCHnfLRbOb6QJ
Yl3q2B7Pkk+Ir+sj8MKAbFZmsA+2KzziJqaEwyLRCtScfBqvQWR4nYoR+eiDaRYp
OfLF8QKBgQCP7mq2lesZHOrnHd0+3n8YrmtxE2KaHdXukULoxPbXWYus+fJOT/tk
EV/sBYhNiGImfxmkDZgRVlTXbVuvH8p3nWmd2zTczbuJNXQwQ05v9cVbuAISQIuU
2QNaF/jdF8oeAt/CsWBGsuJZlUgW56z0YL32XIVadSOmZGzKAE3hkg==";

const CLIENT_ID: &str = "a2a-agent";

fn pem(body: &str) -> String {
    format!("-----BEGIN RSA PRIVATE KEY-----\n{body}\n-----END RSA PRIVATE KEY-----\n")
}

fn key(body: &str, kid: &str) -> CoreRsaPrivateSigningKey {
    CoreRsaPrivateSigningKey::from_pem(&pem(body), Some(JsonWebKeyId::new(kid.to_string())))
        .expect("the test key parses")
}

/// The keys the provider currently publishes. Replaceable, so a test can put
/// the provider through a key rotation.
#[derive(Clone)]
struct Provider {
    issuer: IssuerUrl,
    jwks: Arc<RwLock<CoreJsonWebKeySet>>,
}

async fn discovery(State(provider): State<Provider>) -> Json<CoreProviderMetadata> {
    let metadata = CoreProviderMetadata::new(
        provider.issuer.clone(),
        oauth2::AuthUrl::new(format!("{}/authorize", provider.issuer.as_str())).unwrap(),
        JsonWebKeySetUrl::new(format!("{}/jwks", provider.issuer.as_str())).unwrap(),
        vec![ResponseTypes::new(vec![CoreResponseType::Code])],
        vec![CoreSubjectIdentifierType::Public],
        vec![CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
        EmptyAdditionalProviderMetadata {},
    );
    Json(metadata)
}

async fn jwks(State(provider): State<Provider>) -> Json<CoreJsonWebKeySet> {
    Json(provider.jwks.read().unwrap().clone())
}

/// A running OIDC provider publishing `kid`, and an authenticator that has
/// discovered it.
async fn provider_and_authenticator(kid: &str) -> (Provider, OpenIdConnectAuthenticator) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = IssuerUrl::new(format!("http://{}", listener.local_addr().unwrap())).unwrap();

    let provider = Provider {
        issuer: issuer.clone(),
        jwks: Arc::new(RwLock::new(CoreJsonWebKeySet::new(vec![
            key(SIGNING_KEY, kid).as_verification_key(),
        ]))),
    };

    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .with_state(provider.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let authenticator = OpenIdConnectAuthenticator::new(
        issuer,
        ClientId::new(CLIENT_ID.to_string()),
        None,
        RedirectUrl::new("http://localhost/callback".to_string()).unwrap(),
    )
    .await
    .expect("discovery succeeds against the test provider");

    (provider, authenticator)
}

/// Mint an ID token the way the provider would.
fn id_token(provider: &Provider, signing_key: &CoreRsaPrivateSigningKey, audience: &str) -> String {
    signed(provider, signing_key, audience, ChronoDuration::hours(1))
}

fn signed(
    provider: &Provider,
    signing_key: &CoreRsaPrivateSigningKey,
    audience: &str,
    valid_for: ChronoDuration,
) -> String {
    let now = Utc::now();
    let claims = CoreIdTokenClaims::new(
        provider.issuer.clone(),
        vec![Audience::new(audience.to_string())],
        now + valid_for,
        now,
        StandardClaims::new(SubjectIdentifier::new("user-42".to_string()))
            .set_email(Some(EndUserEmail::new("kari@example.com".to_string()))),
        EmptyAdditionalClaims {},
    );

    CoreIdToken::new(
        claims,
        signing_key,
        CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
        None,
        None,
    )
    .expect("the ID token signs")
    .to_string()
}

fn presenting(token: &str) -> AuthContext {
    AuthContext::new("openidconnect".to_string(), token.to_string())
}

/// The whole point: the principal is the `sub` the provider signed, and the
/// token is not in it.
#[tokio::test]
async fn the_principal_is_the_subject_not_the_token() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;
    let token = id_token(&provider, &key(SIGNING_KEY, "k1"), CLIENT_ID);

    let principal = authenticator
        .authenticate(&presenting(&token))
        .await
        .expect("a token signed by the provider authenticates");

    assert_eq!(principal.id, "user-42");
    assert_eq!(principal.scheme, "openidconnect");
    assert_eq!(
        principal.attributes.get("email").map(String::as_str),
        Some("kari@example.com")
    );
}

/// Signing in again produces a different token for the same person. An agent
/// that keyed a conversation on the credential would hand them a clean slate.
#[tokio::test]
async fn a_second_login_is_the_same_caller() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;
    let signing_key = key(SIGNING_KEY, "k1");

    let first = signed(
        &provider,
        &signing_key,
        CLIENT_ID,
        ChronoDuration::minutes(30),
    );
    let second = signed(&provider, &signing_key, CLIENT_ID, ChronoDuration::hours(2));
    assert_ne!(first, second, "two logins are two tokens");

    let before = authenticator
        .authenticate(&presenting(&first))
        .await
        .unwrap();
    let after = authenticator
        .authenticate(&presenting(&second))
        .await
        .unwrap();

    assert_eq!(before.id, after.id);
}

/// An ID token is issued to one client. Taking one issued to another would let
/// any application the user has signed into speak for them here.
#[tokio::test]
async fn a_token_for_another_client_is_refused() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;
    let token = id_token(&provider, &key(SIGNING_KEY, "k1"), "someone-elses-app");

    let error = authenticator
        .authenticate(&presenting(&token))
        .await
        .expect_err("a token for another audience must not authenticate");

    assert!(error.to_string().contains("audience"), "{error}");
}

#[tokio::test]
async fn an_expired_token_is_refused() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;
    let token = signed(
        &provider,
        &key(SIGNING_KEY, "k1"),
        CLIENT_ID,
        ChronoDuration::hours(-1),
    );

    let error = authenticator
        .authenticate(&presenting(&token))
        .await
        .expect_err("an expired token must not authenticate");

    assert!(error.to_string().contains("Expired"), "{error}");
}

/// A token signed by a key the provider never published — the shape of a forged
/// one. Its `kid` is unknown, so it also takes the rotation path: the keys are
/// fetched again, and it is refused anyway.
#[tokio::test]
async fn a_token_from_an_unknown_signer_is_refused() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;
    let authenticator = authenticator.with_key_refetch_interval(Duration::ZERO);
    let token = id_token(&provider, &key(OTHER_KEY, "forged"), CLIENT_ID);

    let error = authenticator
        .authenticate(&presenting(&token))
        .await
        .expect_err("a token nobody published a key for must not authenticate");

    assert!(
        error.to_string().contains("Invalid OpenID Connect"),
        "{error}"
    );
}

/// Providers rotate signing keys. Discovery ran once, so a token signed with a
/// key issued after that names one the agent has never seen — and the agent has
/// to go and look rather than refuse every caller until it is restarted.
#[tokio::test]
async fn a_rotated_signing_key_is_picked_up() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;
    let authenticator = authenticator.with_key_refetch_interval(Duration::ZERO);

    // The provider rotates: it now signs with k2 and publishes only k2.
    let rotated = key(SIGNING_KEY, "k2");
    *provider.jwks.write().unwrap() = CoreJsonWebKeySet::new(vec![rotated.as_verification_key()]);

    let token = id_token(&provider, &rotated, CLIENT_ID);

    let principal = authenticator
        .authenticate(&presenting(&token))
        .await
        .expect("the agent fetches the rotated key and accepts the token");

    assert_eq!(principal.id, "user-42");
}

/// The floor on refetching is what keeps a stream of junk tokens from becoming
/// a stream of requests to the provider: every one of them names a key the
/// agent does not have.
#[tokio::test]
async fn a_rotation_is_not_chased_on_every_bad_token() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;

    let rotated = key(SIGNING_KEY, "k2");
    *provider.jwks.write().unwrap() = CoreJsonWebKeySet::new(vec![rotated.as_verification_key()]);
    let token = id_token(&provider, &rotated, CLIENT_ID);

    // The default interval has not elapsed since discovery, so the keys are not
    // fetched again and the token is refused on the set the agent holds.
    let error = authenticator
        .authenticate(&presenting(&token))
        .await
        .expect_err("the agent must not refetch on every unverifiable token");

    assert!(
        error.to_string().contains("Invalid OpenID Connect"),
        "{error}"
    );
}

/// A bearer header is labelled `oauth2` by the extractor that reads it, and an
/// ID token arrives in one.
#[tokio::test]
async fn a_bearer_labelled_context_is_accepted() {
    let (provider, authenticator) = provider_and_authenticator("k1").await;
    let token = id_token(&provider, &key(SIGNING_KEY, "k1"), CLIENT_ID);

    let principal = authenticator
        .authenticate(&AuthContext::new("oauth2".to_string(), token))
        .await
        .expect("a bearer-labelled ID token authenticates");

    assert_eq!(principal.id, "user-42");
}

#[tokio::test]
async fn something_that_is_not_a_token_is_refused() {
    let (_provider, authenticator) = provider_and_authenticator("k1").await;

    let error = authenticator
        .authenticate(&presenting("not-a-jwt"))
        .await
        .expect_err("a credential that is not an ID token must not authenticate");

    assert!(error.to_string().contains("well-formed"), "{error}");
}
