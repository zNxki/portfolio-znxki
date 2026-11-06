pub mod contacts;
pub mod experiences;
pub mod media;
pub mod profile;
pub mod projects;
pub mod seed;
pub mod settings;
pub mod site;
pub mod stats;
pub mod technologies;
pub mod users;

use sqlx::PgConnection;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy)]
pub enum Table {
    Contacts,
    Technologies,
    Experiences,
    Projects,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Table::Contacts => "contacts",
            Table::Technologies => "technologies",
            Table::Experiences => "experiences",
            Table::Projects => "projects",
        }
    }
}

pub async fn reorder(conn: &mut PgConnection, table: Table, ids: &[i64]) -> AppResult<()> {
    let name = table.name();
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {name}"))
        .fetch_one(&mut *conn)
        .await?;
    if total != ids.len() as i64 {
        return Err(AppError::BadRequest(
            "L'ordinamento deve includere tutti gli elementi".into(),
        ));
    }
    let positions: Vec<i32> = (0..ids.len() as i32).collect();
    let res = sqlx::query(&format!(
        "UPDATE {name} SET position = v.pos FROM \
         (SELECT * FROM unnest($1::bigint[], $2::int4[]) AS x(id, pos)) v WHERE {name}.id = v.id"
    ))
    .bind(ids.to_vec())
    .bind(positions)
    .execute(&mut *conn)
    .await?;
    if res.rows_affected() != ids.len() as u64 {
        return Err(AppError::BadRequest(
            "Elenco di identificativi non valido".into(),
        ));
    }
    Ok(())
}
