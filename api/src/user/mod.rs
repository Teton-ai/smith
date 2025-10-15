use crate::middlewares::{
    authentication::Auth0UserInfo,
    authorization::{self, AuthorizationConfig},
};
use sqlx::PgPool;

#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub user_id: i32,
    permissions: Vec<authorization::Permission>,
}

impl CurrentUser {
    pub fn has_permission(&self, resource: &str, action: &str) -> bool {
        self.permissions
            .iter()
            .any(|permission| permission.resource == resource && permission.action == action)
    }

    pub async fn id(pg_pool: &PgPool, auth0_sub: &str) -> Result<i32, sqlx::Error> {
        let user_id = sqlx::query!(
            r#"
                SELECT id
                FROM auth.users
                WHERE auth0_user_id = $1
            "#,
            auth0_sub
        )
        .fetch_one(pg_pool)
        .await?
        .id;

        Ok(user_id)
    }

    pub async fn create(
        pg_pool: &PgPool,
        auth0_sub: &str,
        userinfo: Option<Auth0UserInfo>,
    ) -> Result<i32, sqlx::Error> {
        // Insert the user with userinfo if available
        let email = userinfo.as_ref().and_then(|info| info.email.as_ref());
        sqlx::query!(
            r#"
                INSERT INTO auth.users (auth0_user_id, email)
                VALUES ($1, $2)
                ON CONFLICT (auth0_user_id) DO NOTHING
            "#,
            auth0_sub,
            email
        )
        .execute(pg_pool)
        .await?;

        // Now fetch the ID of the newly inserted user
        let user_id = sqlx::query!(
            r#"
                SELECT id
                FROM auth.users
                WHERE auth0_user_id = $1
            "#,
            auth0_sub
        )
        .fetch_one(pg_pool)
        .await?
        .id;

        Ok(user_id)
    }

    pub async fn build(
        pg_pool: &PgPool,
        authorization: &AuthorizationConfig,
        user_id: i32,
    ) -> Result<Self, sqlx::Error> {
        struct UserRole {
            role: String,
        }

        let mut user_roles = sqlx::query_as!(
            UserRole,
            r#"
                    SELECT users_roles.role
                    FROM auth.users
                    LEFT JOIN auth.users_roles ON users_roles.user_id = users.id
                    WHERE users.id = $1
                "#,
            user_id
        )
        .fetch_all(pg_pool)
        .await
        .expect("expected user roles");

        let user_permissions = user_roles
            .iter_mut()
            .filter_map(|user_role| authorization.roles.get(&user_role.role))
            .flat_map(|role| role.permissions.clone())
            .collect();

        let current_user = CurrentUser {
            user_id,
            permissions: user_permissions,
        };

        Ok(current_user)
    }
}
