use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::{
    app::{
        command::create_user::UserWriteRepository,
        query::get_user::{GetUser, UserRepository},
    },
    di::Container,
    error::AppError,
};

#[derive(Serialize, Deserialize)]
struct ErrorResponse {
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".to_owned()),
            AppError::InternalError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".to_owned(),
            ),
        };

        (status, Json(ErrorResponse { message })).into_response()
    }
}

pub struct Server<R, Q>
where
    R: UserWriteRepository,
    Q: UserRepository,
{
    port: u16,
    container: Arc<Container<R, Q>>,
}
impl<R, Q> Server<R, Q>
where
    R: UserWriteRepository + Send + Sync + 'static,
    Q: UserRepository + Send + Sync + 'static,
{
    pub fn new(port: u16, container: Arc<Container<R, Q>>) -> Self {
        Self { port, container }
    }
    pub async fn run(self) {
        let app = get_router(self.container);
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .unwrap();
        axum::serve(listener, app).await.unwrap();
    }
}

async fn get_user<R, Q>(
    State(container): State<Arc<Container<R, Q>>>,
    Path(id): Path<i64>,
) -> Result<Json<GetUser>, AppError>
where
    R: UserWriteRepository + Send + Sync + 'static,
    Q: UserRepository + Send + Sync + 'static,
{
    let user = container.get_user_query.execute(id).await?;
    Ok(Json(user))
}

#[derive(Deserialize, Serialize)]
struct CreateUserRequest {
    username: String,
    password: String,
}

async fn post_user<R, Q>(
    State(container): State<Arc<Container<R, Q>>>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<StatusCode, AppError>
where
    R: UserWriteRepository + Send + Sync + 'static,
    Q: UserRepository + Send + Sync + 'static,
{
    container
        .create_user_command
        .execute(payload.username, payload.password)
        .await?;
    Ok(StatusCode::CREATED)
}

fn get_router<R, Q>(container: Arc<Container<R, Q>>) -> Router
where
    R: UserWriteRepository + Send + Sync + 'static,
    Q: UserRepository + Send + Sync + 'static,
{
    Router::new()
        .route("/users/{id}", axum::routing::get(get_user))
        .route("/users", axum::routing::post(post_user))
        .with_state(container)
}

#[cfg(test)]
mod tests {

    use crate::adapters::postgres::PostgresRepository;

    use super::*;

    use axum::body::Body;
    use sqlx::PgPool;
    use tower::ServiceExt;

    #[sqlx::test]
    async fn test_post_user(pool: PgPool) {
        let repo = PostgresRepository::new(pool.clone());
        let container = Arc::new(Container::new(repo.clone(), repo));
        let app = get_router(container.clone());

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/users")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"newuser","password":"newpassword"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        // create duplicate user
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/users")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"username":"newuser","password":"anotherpassword"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[sqlx::test]
    async fn test_get_router(pool: PgPool) {
        let repo = PostgresRepository::new(pool.clone());
        let container = Arc::new(Container::new(repo.clone(), repo));
        let app = get_router(container.clone());

        let user = container
            .create_user_command
            .execute("testuser".to_owned(), "testuserpassword".to_owned())
            .await
            .unwrap();

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/users/{}", user.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(user.username, "testuser");
    }

    #[sqlx::test]
    async fn test_get_user_not_found(pool: PgPool) {
        // Given
        let repo = PostgresRepository::new(pool.clone());
        let container = Arc::new(Container::new(repo.clone(), repo));
        let app = get_router(container);

        // When
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/users/99999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Then
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn not_found(pool: PgPool) {
        // Given
        let repo = PostgresRepository::new(pool.clone());
        let container = Arc::new(Container::new(repo.clone(), repo));
        let app = get_router(container);

        // When
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/not-found")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Then
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
