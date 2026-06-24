use crate::middlewares::{
    authentication::Auth0UserInfo,
    authorization::{self, AccountsConfig, AuthorizationConfig},
};
use sqlx::PgPool;

pub mod route;

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

    /// Returns (user_id, has_email) if user exists, or RowNotFound if not
    pub async fn lookup(pg_pool: &PgPool, auth0_sub: &str) -> Result<(i32, bool), sqlx::Error> {
        let row = sqlx::query!(
            r#"
                SELECT id, email IS NOT NULL as "has_email!"
                FROM auth.users
                WHERE auth0_user_id = $1
            "#,
            auth0_sub
        )
        .fetch_one(pg_pool)
        .await?;

        Ok((row.id, row.has_email))
    }

    pub async fn update_email(
        pg_pool: &PgPool,
        user_id: i32,
        email: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
                UPDATE auth.users
                SET email = $1, updated_at = now()
                WHERE id = $2
            "#,
            email,
            user_id
        )
        .execute(pg_pool)
        .await?;

        Ok(())
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
                ON CONFLICT (auth0_user_id) DO UPDATE
                SET email = COALESCE(EXCLUDED.email, auth.users.email),
                    updated_at = now()
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

    /// Makes the database match the accounts file. Authoritative: the file is the
    /// sole source of elevated roles, so every non-`default` assignment is dropped
    /// and then re-granted from the file. A user removed from the file is thereby
    /// demoted on the next startup. `default` is never touched — it is the
    /// baseline assigned to every user by the `auth.users` insert trigger.
    ///
    /// A listed email with no user row yet (the user has not logged in) is simply
    /// not matched; `apply_file_role` covers them on first login.
    pub async fn reconcile_roles(
        pg_pool: &PgPool,
        authorization: &AuthorizationConfig,
        accounts: &AccountsConfig,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pg_pool.begin().await?;

        sqlx::query!("DELETE FROM auth.users_roles WHERE role <> 'default'")
            .execute(&mut *tx)
            .await?;

        for (email, role) in &accounts.accounts {
            // `default` is already held by everyone; an undefined role would grant
            // no permissions, so skip it rather than assign a dead row.
            if role == "default" || authorization.permissions_for_role(role).is_empty() {
                continue;
            }
            sqlx::query!(
                r#"
                    INSERT INTO auth.users_roles (user_id, role)
                    SELECT id, $2 FROM auth.users WHERE lower(email) = lower($1)
                    ON CONFLICT (user_id, role) DO NOTHING
                "#,
                email,
                role,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Grants a user the role assigned to their email in the accounts file, if
    /// any. The startup `reconcile_roles` owns revocation; this only closes the
    /// gap where a listed user logs in for the first time between deploys, so
    /// their row did not exist when reconciliation ran. No-op when the email is
    /// not listed.
    pub async fn apply_file_role(
        pg_pool: &PgPool,
        authorization: &AuthorizationConfig,
        accounts: &AccountsConfig,
        user_id: i32,
        email: &str,
    ) -> Result<(), sqlx::Error> {
        let Some(role) = accounts
            .accounts
            .iter()
            .find_map(|(listed, role)| listed.eq_ignore_ascii_case(email).then_some(role))
        else {
            return Ok(());
        };
        if role == "default" || authorization.permissions_for_role(role).is_empty() {
            return Ok(());
        }
        sqlx::query!(
            r#"
                INSERT INTO auth.users_roles (user_id, role)
                VALUES ($1, $2)
                ON CONFLICT (user_id, role) DO NOTHING
            "#,
            user_id,
            role,
        )
        .execute(pg_pool)
        .await?;
        Ok(())
    }

    pub async fn build(
        pg_pool: &PgPool,
        authorization: &AuthorizationConfig,
        user_id: i32,
    ) -> Result<Self, sqlx::Error> {
        struct UserRole {
            // Nullable: the LEFT JOIN yields a NULL role for a user with no
            // assigned roles, so this must be Option to decode without erroring.
            role: Option<String>,
        }

        let user_roles = sqlx::query_as!(
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
        .await?;

        let user_permissions = user_roles
            .iter()
            .filter_map(|user_role| user_role.role.as_deref())
            .flat_map(|role| authorization.permissions_for_role(role))
            .collect();

        let current_user = CurrentUser {
            user_id,
            permissions: user_permissions,
        };

        Ok(current_user)
    }
}
